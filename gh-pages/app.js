const REGISTRY_URL =
  "https://raw.githubusercontent.com/mrx-org/bifti-phantoms/main/registry.json";
const REPO_URL = "https://github.com/mrx-org/bifti-phantoms";

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
  const phantomsSlot = el.querySelector(".phantoms-slot");
  if (phantoms.length > 0) {
    phantomsSlot.appendChild(renderPhantomSection(phantoms, recordId));
  }
  if (recordId) {
    el.querySelector(".files-slot").appendChild(renderFilesCard(recordId));
  }
  return el;
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
    <div class="table-wrap">
      <table class="file-table">
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

function renderPhantomSection(phantoms, recordId) {
  const wrap = document.createElement("div");
  wrap.innerHTML = `<h4 class="phantoms-heading">Phantoms</h4>`;
  const list = document.createElement("div");
  list.className = "phantom-list";
  for (const filename of phantoms) {
    list.appendChild(renderPhantom(filename, recordId));
  }
  wrap.appendChild(list);
  return wrap;
}

function renderPhantom(filename, recordId) {
  const el = document.createElement("details");
  el.className = "card phantom";
  el.innerHTML = `
    <summary class="card-summary">
      <span class="card-title mono">${escape(filename)}</span>
    </summary>
    <div class="card-body">
      <p class="muted">Open to load details&hellip;</p>
    </div>
  `;

  if (!recordId) {
    el.querySelector(".card-body").innerHTML =
      `<p class="muted">No Zenodo record linked for this collection.</p>`;
    return el;
  }

  let loaded = false;
  el.addEventListener("toggle", () => {
    if (!el.open || loaded) return;
    loaded = true;
    const body = el.querySelector(".card-body");
    body.innerHTML = `<p class="muted">Loading&hellip;</p>`;
    const apiUrl = `https://zenodo.org/api/records/${recordId}/files/${filename}/content`;
    const humanUrl = `https://zenodo.org/records/${recordId}/files/${filename}`;
    fetch(apiUrl, { cache: "force-cache" })
      .then((r) => {
        if (!r.ok) throw new Error(`HTTP ${r.status}`);
        return r.json();
      })
      .then((data) => {
        body.innerHTML = renderPhantomData(data, humanUrl);
      })
      .catch((err) => {
        loaded = false;
        body.innerHTML = `
          <p class="error">Could not load phantom (${escape(err.message)}).</p>
          <p class="muted"><a href="${humanUrl}">Open on Zenodo</a></p>`;
      });
  });

  return el;
}

function renderPhantomData(data, sourceUrl) {
  const b0 = data?.system?.B0;
  const tissues = data?.tissues || {};
  const tissueNames = Object.keys(tissues);

  return `
    <dl class="entry-fields">
      ${b0 !== undefined ? `<dt>B<sub>0</sub></dt><dd>${escape(b0)} T</dd>` : ""}
      <dt>Source</dt><dd><a href="${sourceUrl}">Zenodo</a></dd>
    </dl>
    ${tissueNames.length > 0 ? renderTissueTable(tissues, tissueNames) : `<p class="muted">No tissues defined.</p>`}
  `;
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
