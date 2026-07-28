// Registry loading and phantom parsing for the browser. A readable reference
// for working off registry.json (see ../REGISTRY.md), mirroring the data
// layer of python/bifti/src/bifti/registry.py and rust/bifti/src/registry.rs
// - fetch the registry, list/flatten phantoms, resolve a phantom's JSON and
// the NIfTIs it references. No DOM here; app.js owns rendering.

export const REGISTRY_URL =
  "https://raw.githubusercontent.com/mrx-org/bifti-phantoms/main/registry.json";
export const REPO_URL = "https://github.com/mrx-org/bifti-phantoms";

// ===========================================================================
// Public entry points
// ===========================================================================

// Download the latest registry.json from GitHub and return it parsed.
export async function loadRegistry() {
  const res = await fetch(REGISTRY_URL, { cache: "no-cache" });
  if (!res.ok) throw new Error(`HTTP ${res.status}`);
  return res.json();
}

// Every phantom filename in a (possibly nested) phantoms list, depth-first.
// A `phantoms` entry is either a filename string or a group object
// ({ group, phantoms: [...] }); this walks past groups to collect every
// filename regardless of nesting depth.
export function flattenPhantoms(entries) {
  const files = [];
  for (const entry of entries) {
    if (typeof entry === "string") files.push(entry);
    else if (entry && typeof entry === "object") files.push(...flattenPhantoms(entry.phantoms || []));
  }
  return files;
}

// Count of every leaf filename under a (possibly nested) phantoms array.
export function countPhantoms(entries) {
  return flattenPhantoms(entries).length;
}

// Extract the record id from a Zenodo version DOI ("10.5281/zenodo.<id>").
export function parseZenodoRecordId(doi) {
  if (!doi) return null;
  const m = /zenodo\.(\d+)/i.exec(doi);
  return m ? m[1] : null;
}

// Fetch a phantom JSON's raw text from a Zenodo record. Tries configs.tar
// first (one shared download for all phantoms in the record); falls back to
// the direct file URL only if the archive is absent or doesn't contain the
// entry. Raw text (not just the parsed object) is what a zip download needs
// to embed the file byte-for-byte.
export async function fetchPhantomText(recordId, filename) {
  const tarUrl = `https://zenodo.org/api/records/${recordId}/files/configs.tar/content`;
  try {
    const buf = await _fetchArchiveCached(tarUrl);
    const text = _extractFromTar(buf, filename);
    if (text != null) return text;
  } catch (_) {
    // no configs.tar — fall through to direct fetch
  }

  const directUrl = `https://zenodo.org/api/records/${recordId}/files/${encodeURIComponent(filename)}/content`;
  const r = await fetch(directUrl, { cache: "force-cache" });
  if (r.ok) return r.text();

  throw new Error(`${filename} not found in record ${recordId} or configs.tar`);
}

export async function fetchPhantomJson(recordId, filename) {
  return JSON.parse(await fetchPhantomText(recordId, filename));
}

// Every distinct NIfTI filename referenced across all of a phantom's tissues.
// A NIfTI reference is "<filename>[<index>]" (a plain string) or
// { file: "<filename>[<index>]", func: "..." } (a transformed reference).
// Mirrors collect_nifti_files in python/bifti/src/bifti/registry.py and
// rust/bifti/src/registry.rs.
export function collectNiftiFiles(phantom) {
  const files = [];
  const seen = new Set();
  const add = (name) => { if (name && !seen.has(name)) { seen.add(name); files.push(name); } };

  for (const tissue of Object.values(phantom?.tissues || {})) {
    add(_refFile(tissue.density));
    for (const key of ["T1", "T2", "T2'", "ADC", "dB0"]) add(_refFile(tissue[key]));
    for (const key of ["B1+", "B1-"]) {
      for (const channel of tissue[key] || []) add(_refFile(channel));
    }
  }
  return files;
}

// The total download size (bytes) of every file in a Zenodo record.
export async function fetchRecordTotalSize(recordId) {
  const r = await fetch(`https://zenodo.org/api/records/${recordId}`, { cache: "force-cache" });
  if (!r.ok) throw new Error(`HTTP ${r.status}`);
  const data = await r.json();
  return (data?.files || []).reduce((acc, f) => acc + (f.size || 0), 0);
}

// Fetch a phantom's JSON plus every NIfTI it references and bundle them into
// an uncompressed ZIP Blob named "<name>.zip". `onProgress(done, total)` is
// called after each file (the phantom JSON counts as the first).
export async function buildPhantomZip(recordId, filename, onProgress) {
  const text = await fetchPhantomText(recordId, filename);
  const niftiFiles = collectNiftiFiles(JSON.parse(text));
  const total = 1 + niftiFiles.length;
  let done = 1; // the phantom JSON itself
  if (onProgress) onProgress(done, total);

  const entries = [{ name: filename, data: new TextEncoder().encode(text) }];
  for (const niftiName of niftiFiles) {
    entries.push({ name: niftiName, data: await _fetchNiftiBytesCached(recordId, niftiName) });
    done++;
    if (onProgress) onProgress(done, total);
  }

  return {
    blob: buildZip(entries),
    downloadName: `${filename.replace(/\.json$/i, "")}.zip`,
  };
}

// ===========================================================================
// Internals
// ===========================================================================

// One shared Promise<ArrayBuffer> per archive URL so a collection with many
// phantoms only triggers one configs.tar download regardless of concurrency.
const _archiveCache = new Map();
function _fetchArchiveCached(url) {
  if (!_archiveCache.has(url)) {
    _archiveCache.set(
      url,
      fetch(url, { cache: "force-cache" }).then((r) =>
        r.ok ? r.arrayBuffer() : Promise.reject(new Error(`HTTP ${r.status}`))
      )
    );
  }
  return _archiveCache.get(url);
}

// Walk a TAR ArrayBuffer (512-byte blocks) and return the text content of
// `filename`, or null if the entry is not found.
function _extractFromTar(buffer, filename) {
  const view = new Uint8Array(buffer);
  const dec = new TextDecoder();
  let offset = 0;
  while (offset + 512 <= view.length) {
    const name = dec.decode(view.subarray(offset, offset + 100)).replace(/\0/g, "");
    if (!name) break; // end-of-archive null block
    const size = parseInt(dec.decode(view.subarray(offset + 124, offset + 136)).trim(), 8);
    if (name === filename) {
      return dec.decode(view.subarray(offset + 512, offset + 512 + size));
    }
    offset += 512 + Math.ceil(size / 512) * 512;
  }
  return null;
}

function _niftiFilenameFromRef(ref) {
  const m = /^(.*)\[\d+\]$/.exec(ref);
  return m ? m[1] : ref;
}

function _refFile(prop) {
  if (prop == null || typeof prop === "number") return null;
  if (typeof prop === "string") return _niftiFilenameFromRef(prop);
  if (typeof prop === "object" && typeof prop.file === "string") return _niftiFilenameFromRef(prop.file);
  return null;
}

// One shared Promise<Uint8Array> per (record, filename), so downloading
// several configs that reference the same NIfTI (a shared B1 map, a shared
// density volume, ...) only fetches it from Zenodo once per page visit.
const _niftiCache = new Map();
function _fetchNiftiBytesCached(recordId, filename) {
  const key = `${recordId}/${filename}`;
  if (!_niftiCache.has(key)) {
    const url = `https://zenodo.org/api/records/${recordId}/files/${encodeURIComponent(filename)}/content`;
    _niftiCache.set(
      key,
      fetch(url, { cache: "force-cache" }).then((r) =>
        r.ok
          ? r.arrayBuffer().then((buf) => new Uint8Array(buf))
          : Promise.reject(new Error(`HTTP ${r.status} fetching ${filename}`))
      )
    );
  }
  return _niftiCache.get(key);
}

const _CRC_TABLE = (() => {
  const table = new Uint32Array(256);
  for (let n = 0; n < 256; n++) {
    let c = n;
    for (let k = 0; k < 8; k++) c = (c & 1) ? (0xedb88320 ^ (c >>> 1)) : (c >>> 1);
    table[n] = c >>> 0;
  }
  return table;
})();

function _crc32(bytes) {
  let crc = 0xffffffff;
  for (let i = 0; i < bytes.length; i++) crc = _CRC_TABLE[(crc ^ bytes[i]) & 0xff] ^ (crc >>> 8);
  return (crc ^ 0xffffffff) >>> 0;
}

// Build an uncompressed (store-method) ZIP from [{ name, data: Uint8Array }].
// No compression library needed - ZIP allows raw stored entries, so this is
// just the local/central-directory bookkeeping plus a CRC-32 per entry.
function buildZip(files) {
  const encoder = new TextEncoder();
  const localParts = [];
  const centralParts = [];
  let offset = 0;

  for (const { name, data } of files) {
    const nameBytes = encoder.encode(name);
    const crc = _crc32(data);

    const local = new DataView(new ArrayBuffer(30));
    local.setUint32(0, 0x04034b50, true);
    local.setUint16(4, 20, true); // version needed
    local.setUint16(6, 0, true); // flags
    local.setUint16(8, 0, true); // method: store
    local.setUint16(10, 0, true); // mod time
    local.setUint16(12, 0, true); // mod date
    local.setUint32(14, crc, true);
    local.setUint32(18, data.length, true); // compressed size
    local.setUint32(22, data.length, true); // uncompressed size
    local.setUint16(26, nameBytes.length, true);
    local.setUint16(28, 0, true); // extra length
    localParts.push(new Uint8Array(local.buffer), nameBytes, data);

    const central = new DataView(new ArrayBuffer(46));
    central.setUint32(0, 0x02014b50, true);
    central.setUint16(4, 20, true); // version made by
    central.setUint16(6, 20, true); // version needed
    central.setUint16(8, 0, true); // flags
    central.setUint16(10, 0, true); // method
    central.setUint16(12, 0, true); // mod time
    central.setUint16(14, 0, true); // mod date
    central.setUint32(16, crc, true);
    central.setUint32(20, data.length, true);
    central.setUint32(24, data.length, true);
    central.setUint16(28, nameBytes.length, true);
    central.setUint16(30, 0, true); // extra length
    central.setUint16(32, 0, true); // comment length
    central.setUint16(34, 0, true); // disk number start
    central.setUint16(36, 0, true); // internal attrs
    central.setUint32(38, 0, true); // external attrs
    central.setUint32(42, offset, true); // offset of local header
    centralParts.push(new Uint8Array(central.buffer), nameBytes);

    offset += 30 + nameBytes.length + data.length;
  }

  const centralStart = offset;
  const centralSize = centralParts.reduce((acc, p) => acc + p.length, 0);

  const end = new DataView(new ArrayBuffer(22));
  end.setUint32(0, 0x06054b50, true);
  end.setUint16(4, 0, true); // disk number
  end.setUint16(6, 0, true); // disk where central directory starts
  end.setUint16(8, files.length, true);
  end.setUint16(10, files.length, true);
  end.setUint32(12, centralSize, true);
  end.setUint32(16, centralStart, true);
  end.setUint16(20, 0, true); // comment length

  return new Blob([...localParts, ...centralParts, new Uint8Array(end.buffer)], { type: "application/zip" });
}
