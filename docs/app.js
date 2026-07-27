const REGISTRY_URL =
  "https://raw.githubusercontent.com/mrx-org/bifti-phantoms/grouped-phantoms/registry.json";
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
  // Reverse of registry.json's order, so the newest-added collections show first.
  const entries = Object.entries(data).reverse();
  if (entries.length === 0) {
    container.innerHTML = `<p class="muted">No entries yet.</p>`;
    return;
  }
  container.innerHTML = "";

  // Collect all unique tags across all entries, sorted
  const allTags = [...new Set(
    entries.flatMap(([, e]) => Array.isArray(e.keywords) ? e.keywords : [])
  )].sort();

  const activeTags = new Set();
  const cards = [];

  function applyFilter() {
    for (const btn of filterBar.querySelectorAll(".tag-filter-btn")) {
      btn.classList.toggle("active", activeTags.has(btn.dataset.tag));
    }
    for (const { el, keywords } of cards) {
      el.hidden = activeTags.size > 0 && ![...activeTags].every((t) => keywords.includes(t));
    }
  }

  const filterBar = document.createElement("div");
  filterBar.className = "tag-filter";
  filterBar.textContent = "Tags:"
  for (const tag of allTags) {
    const btn = document.createElement("button");
    btn.className = "tag-filter-btn";
    btn.dataset.tag = tag;
    btn.textContent = tag;
    btn.addEventListener("click", () => {
      if (activeTags.has(tag)) activeTags.delete(tag);
      else activeTags.add(tag);
      applyFilter();
    });
    filterBar.appendChild(btn);
  }
  if (allTags.length > 0) container.appendChild(filterBar);

  for (const [name, entry] of entries) {
    const el = renderEntry(name, entry);
    const keywords = Array.isArray(entry.keywords) ? entry.keywords : [];
    cards.push({ el, keywords });
    container.appendChild(el);
  }
}

const TISSUE_PROPERTIES = ["T1", "T2", "T2'", "ADC", "dB0", "B1+", "B1-"];
const ARRAY_PROPERTIES = new Set(["B1+", "B1-"]);


function renderEntry(name, entry) {
  const phantoms = Array.isArray(entry.phantoms) ? entry.phantoms : [];
  const phantomCount = countPhantoms(phantoms);
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
      ${renderTags(entry.keywords)}
      <span class="card-meta">${phantomCount} phantom${phantomCount === 1 ? "" : "s"}</span>
    </summary>
    <div class="card-body">
      ${entry.description ? `<p class="entry-desc">${escape(entry.description)}</p>` : ""}
      <dl class="entry-fields">
        ${authors ? `<dt>Authors</dt><dd>${escape(authors)}</dd>` : ""}
        ${entry.license ? `<dt>License</dt><dd>${escape(entry.license)}</dd>` : ""}
        </dl>
      <div class="phantoms-slot"></div>
      ${doiUrl ? `<hr class="files-divider">` : ""}
      <div class="files-slot"></div>
    </div>
  `;

  let phantomSection = null;
  if (phantoms.length > 0) {
    phantomSection = renderPhantomSection(phantoms, recordId, name);
    el.querySelector(".phantoms-slot").appendChild(phantomSection);
  }

  let filesSummary = null;
  if (doiUrl) {
    filesSummary = renderFilesSummary(doiUrl, entry.doi, recordId);
    el.querySelector(".files-slot").appendChild(filesSummary);
  }

  let loaded = false;
  el.addEventListener("toggle", () => {
    if (!el.open || loaded) return;
    loaded = true;
    if (phantomSection) phantomSection.loadPhantoms();
    if (filesSummary) filesSummary.load();
  });

  return el;
}

// Top-level entry point: wraps a (possibly nested) phantoms array in a
// '.list-section' and forwards lazy-loading to the tree it renders.
function renderPhantomSection(phantoms, recordId, collectionName) {
  const wrap = document.createElement("div");
  wrap.className = "list-section";
  const tree = renderPhantomTree(phantoms, recordId, collectionName);
  wrap.appendChild(tree);
  wrap.loadPhantoms = tree.loadPhantoms;
  return wrap;
}

// Renders one level of a (possibly nested) phantoms array: leaf filenames
// become rows in a table; group objects become nested accordions (see
// renderPhantomGroup) that lazily render their own subtree on first expand.
// `pathLabel` is the breadcrumb (collection + group names so far) shown in
// each phantom's tissue modal header.
function renderPhantomTree(entries, recordId, pathLabel) {
  const container = document.createElement("div");
  container.className = "phantom-tree";

  const files = entries.filter((e) => typeof e === "string");
  const groups = entries.filter((e) => e && typeof e === "object");

  let table = null;
  if (files.length > 0) {
    table = renderPhantomTable(files, recordId, pathLabel);
    container.appendChild(table);
  }
  for (const group of groups) {
    container.appendChild(renderPhantomGroup(group, recordId, pathLabel));
  }

  // Only the leaf table has anything to fetch at this level - nested groups
  // load themselves lazily when expanded (see renderPhantomGroup).
  container.loadPhantoms = () => { if (table) table.loadPhantoms(); };

  return container;
}

// A named group of phantom entries, rendered as a nested collapsible card
// that drills into its own subtree. Lazily loads phantom metadata (B0,
// resolution, tissues) for its subtree only the first time it's expanded -
// important since a deep tree can otherwise trigger hundreds of Zenodo
// fetches on page load.
function renderPhantomGroup(group, recordId, pathLabel) {
  const groupPhantoms = Array.isArray(group.phantoms) ? group.phantoms : [];
  const count = countPhantoms(groupPhantoms);
  const childPathLabel = `${pathLabel}/${group.group}`;

  const details = document.createElement("details");
  details.className = "card group";
  details.innerHTML = `
    <summary class="card-summary">
      <span class="card-title">${escape(group.group)}</span>
      ${group.default ? `<span class="tag group-default-tag">default: ${escape(group.default)}</span>` : ""}
      <span class="card-meta">${count} phantom${count === 1 ? "" : "s"}</span>
    </summary>
    <div class="card-body">
      ${group.description ? `<p class="entry-desc">${escape(group.description)}</p>` : ""}
      <div class="group-content-slot"></div>
    </div>
  `;

  const tree = renderPhantomTree(groupPhantoms, recordId, childPathLabel);
  details.querySelector(".group-content-slot").appendChild(tree);

  let loaded = false;
  details.addEventListener("toggle", () => {
    if (!details.open || loaded) return;
    loaded = true;
    tree.loadPhantoms();
  });

  return details;
}

// Every leaf filename under a (possibly nested) phantoms array.
function countPhantoms(entries) {
  let n = 0;
  for (const e of entries) {
    if (typeof e === "string") n++;
    else if (e && typeof e === "object") n += countPhantoms(e.phantoms || []);
  }
  return n;
}

function renderPhantomTable(phantoms, recordId, pathLabel) {
  const tableWrap = document.createElement("div");
  tableWrap.className = "data-list-wrap";

  const table = document.createElement("table");
  table.className = "data-list";

  const thead = document.createElement("thead");
  thead.innerHTML = `<tr>
    <th>Phantom</th>
    <th>B<sub>0</sub></th>
    <th>Tissues</th>
    <th class="col-spacer"></th>
    <th>Resolution</th>
  </tr>`;
  table.appendChild(thead);

  const tbody = document.createElement("tbody");

  const rows = phantoms.map((filename) => {
    const tr = document.createElement("tr");

    const filenameTd = document.createElement("td");
    filenameTd.className = "col-name";
    const filenameCode = document.createElement("code");
    filenameCode.textContent = filename.replace(/\.json$/i, "");
    filenameTd.appendChild(filenameCode);
    tr.appendChild(filenameTd);

    const b0Td = document.createElement("td");
    b0Td.innerHTML = '<span class="loading-text">…</span>';
    tr.appendChild(b0Td);

    const tissueTd = document.createElement("td");
    tissueTd.className = "col-muted";
    tissueTd.innerHTML = '<span class="loading-text">…</span>';
    tr.appendChild(tissueTd);

    const spacerTd = document.createElement("td");
    spacerTd.className = "col-spacer";
    tr.appendChild(spacerTd);

    const resTd = document.createElement("td");
    resTd.innerHTML = '<span class="loading-text">…</span>';
    tr.appendChild(resTd);

    tbody.appendChild(tr);

    return { filename, filenameTd, b0Td, resTd, tissueTd };
  });

  table.appendChild(tbody);
  tableWrap.appendChild(table);

  tableWrap.loadPhantoms = () => {
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
          btn.textContent = filename.replace(/\.json$/i, "");
          btn.title = "View tissues";
          btn.addEventListener("click", () => openTissueModal(tissues, data, filename, pathLabel));
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

  return tableWrap;
}

function openTissueModal(tissues, rawData, filename, pathLabel) {
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
  titleEl.innerHTML = `<span class="modal-path-collection">${escape(pathLabel)}/</span><span class="modal-path-file">${escape(filename)}</span>`;

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

// Just the doi link plus the record's total download size - no per-file listing.
function renderFilesSummary(doiUrl, doi, recordId) {
  const el = document.createElement("p");
  el.className = "files-label";
  el.innerHTML = `Raw files: <a href="${doiUrl}">${escape(doi)}</a>`;

  el.load = () => {
    if (!recordId) return;
    fetch(`https://zenodo.org/api/records/${recordId}`, { cache: "force-cache" })
      .then((r) => {
        if (!r.ok) throw new Error(`HTTP ${r.status}`);
        return r.json();
      })
      .then((data) => {
        const total = (data?.files || []).reduce((acc, f) => acc + (f.size || 0), 0);
        el.innerHTML = `Raw files: <a href="${doiUrl}">${escape(doi)}</a> - ${escape(formatSize(total))}`;
      })
      .catch(() => {
        // leave the doi link without a size on failure
      });
  };

  return el;
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
  return `<div class="tags">${keywords.map((k) => `<span class="tag">${escape(k)}</span>`).join("")}</div>`;
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
        return `<td>${renderCell(t?.[p], ARRAY_PROPERTIES.has(p))}</td>`;
      }).join("");
      return `<tr><th scope="row">${escape(name)}</th>${cells}</tr>`;
    })
    .join("");
  return `<div class="table-wrap"><table class="tissue-table">${head}<tbody>${rows}</tbody></table></div>`;
}

function renderCell(val, isArrayProp) {
  if (val == null) return '<span class="cell-missing">-</span>';
  if (isArrayProp) {
    const arr = Array.isArray(val) ? val : [val];
    return arr.length === 1 ? renderVal(arr[0]) : renderArray(arr);
  }
  return renderVal(val);
}

function renderVal(val) {
  if (val == null) return '<span class="cell-missing">-</span>';
  if (typeof val === "number") return escape(String(val));
  if (typeof val === "string") return escape(val);
  if (typeof val === "object" && val.file) {
    const tip = val.func ? `${val.file} → ${val.func}` : val.file;
    return `<span class="cell-ref" data-tooltip="${escape(tip)}">mapped</span>`;
  }
  return escape(JSON.stringify(val));
}

function renderArray(arr) {
  return arr.map((v, i) => {
    if (typeof v === "number") return escape(String(v));
    const label = `c${i + 1}`;
    const tip = typeof v === "string" ? v
      : (v && v.file ? (v.func ? `${v.file} → ${v.func}` : v.file) : JSON.stringify(v));
    return `<span class="cell-ref" data-tooltip="${escape(tip)}">${label}</span>`;
  }).join(", ");
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
