const REGISTRY_URL =
  "https://raw.githubusercontent.com/mrx-org/bifti-phantoms/main/registry.json";
const REPO_URL = "https://github.com/mrx-org/bifti-phantoms";

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

// Fetch a phantom JSON from a Zenodo record. Tries configs.tar first (one
// shared download for all phantoms in the record); falls back to the direct
// file URL only if the archive is absent or doesn't contain the entry.
async function fetchPhantomJson(recordId, filename) {
  const tarUrl = `https://zenodo.org/api/records/${recordId}/files/configs.tar/content`;
  try {
    const buf = await _fetchArchiveCached(tarUrl);
    const text = _extractFromTar(buf, filename);
    if (text != null) return JSON.parse(text);
  } catch (_) {
    // no configs.tar — fall through to direct fetch
  }

  const directUrl = `https://zenodo.org/api/records/${recordId}/files/${encodeURIComponent(filename)}/content`;
  const r = await fetch(directUrl, { cache: "force-cache" });
  if (r.ok) return r.json();

  throw new Error(`${filename} not found in record ${recordId} or configs.tar`);
}

async function loadRegistry() {
  const container = document.getElementById("registry-list");
  try {
    const res = await fetch(REGISTRY_URL, { cache: "no-cache" });
    if (!res.ok) throw new Error(`HTTP ${res.status}`);
    const data = await res.json();
    renderRegistry(container, data);
  } catch (err) {
    container.innerHTML = `
      <p class="error">
        Could not load the registry (${err.message}).
        See <a href="${REPO_URL}/blob/main/registry.json">registry.json</a> on GitHub.
      </p>`;
  }
}

function renderRegistry(container, data) {
  const entries = Object.entries(data);
  if (entries.length === 0) {
    container.innerHTML = `<p class="muted">No entries yet.</p>`;
    return;
  }
  container.innerHTML = "";
  for (const [name, entry] of entries) {
    container.appendChild(renderEntry(name, entry));
  }
}

const TISSUE_PROPERTIES = ["T1", "T2", "T2'", "ADC", "dB0", "B1+", "B1-"];
const ARRAY_PROPERTIES = new Set(["B1+", "B1-"]);

const GLYPHS = {
  missing: '<i class="fa-solid fa-ban"></i>',
  number: '<i class="fa-solid fa-pen"></i>',
  file: '<i class="fa-regular fa-file"></i>',
  mapping: '<i class="fa-solid fa-calculator"></i>',
};

function renderEntry(name, entry) {
  const phantoms = Array.isArray(entry.phantoms) ? entry.phantoms : [];
  const authors = (entry.authors || [])
    .map((a) => a.name)
    .filter(Boolean)
    .join(", ");
  const doiUrl = entry.doi ? `https://doi.org/${entry.doi}` : null;
  const recordId = parseZenodoRecordId(entry.doi);

  const el = document.createElement("details");
  el.className = "card collection";
  el.innerHTML = `
    <summary class="card-summary">
      <span class="card-title">${escape(name)}</span>
      <span class="card-meta">${phantoms.length} phantom${phantoms.length === 1 ? "" : "s"}</span>
    </summary>
    <div class="card-body">
      ${entry.description ? `<p class="entry-desc">${escape(entry.description)}</p>` : ""}
      ${renderTags(entry.keywords)}
      <dl class="entry-fields">
        ${authors ? `<dt>Authors</dt><dd>${escape(authors)}</dd>` : ""}
        ${entry.license ? `<dt>License</dt><dd>${escape(entry.license)}</dd>` : ""}
        ${doiUrl ? `<dt>DOI</dt><dd><a href="${doiUrl}">${escape(entry.doi)}</a></dd>` : ""}
      </dl>
      <div class="phantoms-slot"></div>
      <div class="files-slot"></div>
    </div>
  `;

  let phantomSection = null;
  if (phantoms.length > 0) {
    phantomSection = renderPhantomSection(phantoms, recordId, name);
    el.querySelector(".phantoms-slot").appendChild(phantomSection);
  }

  if (recordId) {
    el.querySelector(".files-slot").appendChild(renderFilesCard(recordId));
  }

  let phantomsStarted = false;
  el.addEventListener("toggle", () => {
    if (!el.open || phantomsStarted) return;
    phantomsStarted = true;
    if (phantomSection) phantomSection.loadPhantoms();
  });

  return el;
}

function renderPhantomSection(phantoms, recordId, collectionName) {
  const wrap = document.createElement("div");
  wrap.className = "phantom-table-section";

  const tableWrap = document.createElement("div");
  tableWrap.className = "table-wrap phantom-list-wrap";

  const table = document.createElement("table");
  table.className = "phantom-table";

  const thead = document.createElement("thead");
  thead.innerHTML = `<tr>
    <th>File</th>
    <th>B<sub>0</sub></th>
    <th>Resolution</th>
    <th>Tissues</th>
    <th class="col-spacer"></th>
  </tr>`;
  table.appendChild(thead);

  const tbody = document.createElement("tbody");

  const rows = phantoms.map((filename) => {
    const tr = document.createElement("tr");

    const filenameTd = document.createElement("td");
    filenameTd.className = "phantom-filename";
    const filenameCode = document.createElement("code");
    filenameCode.textContent = filename;
    filenameTd.appendChild(filenameCode);
    tr.appendChild(filenameTd);

    const b0Td = document.createElement("td");
    b0Td.innerHTML = '<span class="loading-text">…</span>';
    tr.appendChild(b0Td);

    const resTd = document.createElement("td");
    resTd.innerHTML = '<span class="loading-text">…</span>';
    tr.appendChild(resTd);

    const tissueTd = document.createElement("td");
    tissueTd.className = "tissue-names-cell";
    tissueTd.innerHTML = '<span class="loading-text">…</span>';
    tr.appendChild(tissueTd);

    const spacerTd = document.createElement("td");
    spacerTd.className = "col-spacer";
    tr.appendChild(spacerTd);

    tbody.appendChild(tr);

    return { filename, filenameTd, b0Td, resTd, tissueTd };
  });

  table.appendChild(tbody);
  tableWrap.appendChild(table);
  wrap.appendChild(tableWrap);

  wrap.loadPhantoms = () => {
    for (const { filename, filenameTd, b0Td, resTd, tissueTd } of rows) {
      if (!recordId) {
        const dash = '<span class="muted">—</span>';
        b0Td.innerHTML = dash;
        resTd.innerHTML = dash;
        tissueTd.innerHTML = dash;
        continue;
      }

      fetchPhantomJson(recordId, filename)
        .then((data) => {
          const b0 = data?.system?.B0;
          b0Td.textContent = b0 !== undefined ? `${b0} T` : "—";

          const res = data?.reslice_to?.resolution;
          resTd.textContent = Array.isArray(res) ? res.join("×") : "native";

          const tissues = data?.tissues || {};
          const tissueNames = Object.keys(tissues);
          tissueTd.textContent = tissueNames.length > 0 ? tissueNames.join(", ") : "—";

          const btn = document.createElement("button");
          btn.className = "filename-link";
          btn.textContent = filename;
          btn.title = "View tissues";
          btn.addEventListener("click", () => openTissueModal(tissues, data, filename, collectionName));
          filenameTd.innerHTML = "";
          filenameTd.appendChild(btn);
        })
        .catch((err) => {
          const errHtml = `<span class="muted" title="${escape(err.message)}">!</span>`;
          b0Td.innerHTML = errHtml;
          resTd.innerHTML = errHtml;
          tissueTd.innerHTML = errHtml;
        });
    }
  };

  return wrap;
}

function openTissueModal(tissues, rawData, filename, collectionName) {
  const overlay = document.createElement("div");
  overlay.className = "modal-overlay";
  overlay.setAttribute("role", "dialog");
  overlay.setAttribute("aria-modal", "true");

  const box = document.createElement("div");
  box.className = "modal-box";

  const header = document.createElement("div");
  header.className = "modal-header";

  // Left: toggle switch + label
  const toggleWrap = document.createElement("div");
  toggleWrap.className = "view-toggle";

  const toggleBtn = document.createElement("button");
  toggleBtn.className = "view-toggle-btn";
  toggleBtn.setAttribute("role", "switch");
  toggleBtn.setAttribute("aria-checked", "false");
  toggleBtn.setAttribute("aria-label", "Switch between table and JSON view");
  toggleBtn.innerHTML = `<span class="view-toggle-track"><span class="view-toggle-thumb"></span></span>`;

  const toggleLabel = document.createElement("span");
  toggleLabel.className = "view-toggle-label";
  toggleLabel.textContent = "table";

  toggleWrap.appendChild(toggleBtn);
  toggleWrap.appendChild(toggleLabel);

  // Center: plain path, non-interactive
  const titleEl = document.createElement("span");
  titleEl.className = "modal-header-title";
  titleEl.innerHTML = `<span class="modal-path-collection">${escape(collectionName)}/</span><span class="modal-path-file">${escape(filename)}</span>`;

  // Right: close button
  const closeBtn = document.createElement("button");
  closeBtn.className = "modal-close";
  closeBtn.setAttribute("aria-label", "Close");
  closeBtn.textContent = "×";

  header.appendChild(toggleWrap);
  header.appendChild(titleEl);
  header.appendChild(closeBtn);

  const body = document.createElement("div");
  body.className = "modal-body";

  const tissueNames = Object.keys(tissues);

  function showTable() {
    body.innerHTML = tissueNames.length > 0
      ? renderTissueTable(tissues, tissueNames)
      : `<p class="muted" style="padding:1rem">No tissues defined.</p>`;
    toggleBtn.setAttribute("aria-checked", "false");
    toggleLabel.textContent = "table";
  }

  function showJson() {
    const pre = document.createElement("pre");
    pre.className = "json-viewer";
    pre.innerHTML = highlightJson(rawData);
    body.innerHTML = "";
    body.appendChild(pre);
    toggleBtn.setAttribute("aria-checked", "true");
    toggleLabel.textContent = "json";
  }

  let showingJson = false;
  showTable();

  toggleBtn.addEventListener("click", () => {
    showingJson = !showingJson;
    showingJson ? showJson() : showTable();
  });

  box.appendChild(header);
  box.appendChild(body);
  overlay.appendChild(box);
  document.body.appendChild(overlay);

  const close = () => {
    overlay.remove();
    document.removeEventListener("keydown", onKey);
  };

  const onKey = (e) => { if (e.key === "Escape") close(); };

  overlay.addEventListener("click", (e) => { if (e.target === overlay) close(); });
  closeBtn.addEventListener("click", close);
  document.addEventListener("keydown", onKey);
  closeBtn.focus();
}

function highlightJson(obj) {
  const json = JSON.stringify(obj, null, 2);
  // Walk the string, escaping non-tokens and wrapping tokens in spans.
  const tokenRe = /("(?:[^"\\]|\\.)*")\s*:|("(?:[^"\\]|\\.)*")|(true|false|null)|(-?\d+(?:\.\d+)?(?:[eE][+-]?\d+)?)/g;
  let out = "";
  let last = 0;
  let m;
  while ((m = tokenRe.exec(json)) !== null) {
    out += escape(json.slice(last, m.index));
    const [full, key, str, kw, num] = m;
    if (key !== undefined) {
      out += `<span class="json-key">${escape(key)}</span>:`;
    } else if (str !== undefined) {
      out += `<span class="json-str">${escape(str)}</span>`;
    } else if (kw !== undefined) {
      out += `<span class="json-bool">${kw}</span>`;
    } else {
      out += `<span class="json-num">${num}</span>`;
    }
    last = m.index + full.length;
  }
  out += escape(json.slice(last));
  return out;
}

function renderFilesCard(recordId) {
  const el = document.createElement("details");
  el.className = "card files-card";
  el.innerHTML = `
    <summary class="card-summary">
      <span class="card-title">All files on Zenodo</span>
    </summary>
    <div class="card-body">
      <p class="muted">Open to load file list&hellip;</p>
    </div>
  `;

  let loaded = false;
  el.addEventListener("toggle", () => {
    if (!el.open || loaded) return;
    loaded = true;
    const body = el.querySelector(".card-body");
    body.innerHTML = `<p class="muted">Loading&hellip;</p>`;
    fetch(`https://zenodo.org/api/records/${recordId}`, { cache: "force-cache" })
      .then((r) => {
        if (!r.ok) throw new Error(`HTTP ${r.status}`);
        return r.json();
      })
      .then((data) => {
        body.innerHTML = renderFileList(data?.files || [], recordId);
      })
      .catch((err) => {
        loaded = false;
        body.innerHTML = `<p class="error">Could not load file list (${escape(err.message)}).</p>`;
      });
  });

  return el;
}

function renderFileList(files, recordId) {
  if (files.length === 0) return `<p class="muted">No files.</p>`;
  const total = files.reduce((acc, f) => acc + (f.size || 0), 0);
  const rows = files
    .map((f) => {
      const url = `https://zenodo.org/records/${recordId}/files/${encodeURIComponent(f.key)}`;
      return `
        <tr>
          <td><a href="${url}"><code>${escape(f.key)}</code></a></td>
          <td class="size">${escape(formatSize(f.size))}</td>
        </tr>`;
    })
    .join("");
  return `
    <div class="table-wrap file-list-wrap">
      <table class="file-table">
        <thead>
          <tr><th>File</th><th class="size">Size</th></tr>
        </thead>
        <tbody>${rows}</tbody>
        <tfoot>
          <tr>
            <th>${files.length} file${files.length === 1 ? "" : "s"}</th>
            <th class="size">${escape(formatSize(total))}</th>
          </tr>
        </tfoot>
      </table>
    </div>
  `;
}

function formatSize(bytes) {
  if (bytes === undefined || bytes === null) return "";
  if (bytes < 1024) return `${bytes} B`;
  const units = ["KB", "MB", "GB", "TB"];
  let val = bytes / 1024;
  let i = 0;
  while (val >= 1024 && i < units.length - 1) {
    val /= 1024;
    i++;
  }
  return `${val.toFixed(val < 10 ? 2 : 1)} ${units[i]}`;
}

function renderTags(keywords) {
  if (!Array.isArray(keywords) || keywords.length === 0) return "";
  return `<div class="tags">${keywords
    .map((k) => `<span class="tag">${escape(k)}</span>`)
    .join("")}</div>`;
}

function renderTissueTable(tissues, names) {
  const head = `
    <thead>
      <tr>
        <th>Tissue</th>
        ${TISSUE_PROPERTIES.map((p) => `<th>${escape(p)}</th>`).join("")}
      </tr>
    </thead>`;
  const rows = names
    .map((name) => {
      const t = tissues[name];
      const cells = TISSUE_PROPERTIES.map((p) => {
        return `<td>${formatCell(t?.[p], ARRAY_PROPERTIES.has(p))}</td>`;
      }).join("");
      return `<tr><th scope="row">${escape(name)}</th>${cells}</tr>`;
    })
    .join("");
  return `<div class="table-wrap"><table class="tissue-table">${head}<tbody>${rows}</tbody></table></div>`;
}

function formatCell(val, isArrayProp) {
  if (isArrayProp) {
    if (val === undefined) return missingGlyph();
    const arr = Array.isArray(val) ? val : [val];
    return `<span class="array-cell">${arr.map(formatGlyph).join(", ")}</span>`;
  }
  if (val === undefined) return missingGlyph();
  return formatGlyph(val);
}

function formatGlyph(val) {
  if (val === undefined || val === null) return missingGlyph();
  if (typeof val === "number") {
    return `<span class="glyph" data-tooltip="${escape(val)}">${GLYPHS.number}</span>`;
  }
  if (typeof val === "string") {
    return `<span class="glyph" data-tooltip="${escape(val)}">${GLYPHS.file}</span>`;
  }
  if (typeof val === "object" && val.file) {
    const title = `${val.file}${val.func ? "  →  " + val.func : ""}`;
    return `<span class="glyph" data-tooltip="${escape(title)}">${GLYPHS.mapping}</span>`;
  }
  return `<span class="glyph" data-tooltip="${escape(JSON.stringify(val))}">?</span>`;
}

function missingGlyph() {
  return `<span class="glyph muted" data-tooltip="missing">${GLYPHS.missing}</span>`;
}

function parseZenodoRecordId(doi) {
  if (!doi) return null;
  const m = /zenodo\.(\d+)/i.exec(doi);
  return m ? m[1] : null;
}

function escape(s) {
  return String(s)
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

loadRegistry();
