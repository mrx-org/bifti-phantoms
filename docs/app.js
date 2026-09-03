import {
  REPO_URL,
  loadRegistry as fetchRegistry,
  loadCatalog as fetchCatalog,
  countPhantoms,
  parseZenodoRecordId,
  fetchPhantomJson,
  fetchRecordTotalSize,
  buildPhantomZip,
} from "./bifti.js";

// Downloads <name>.zip (same base name as the phantom JSON) containing the
// JSON plus every NIfTI it references, bundled client-side.
async function downloadPhantomZip(recordId, filename, button) {
  const original = button.textContent;
  button.disabled = true;
  try {
    const { blob, downloadName } = await buildPhantomZip(recordId, filename, (done, total) => {
      button.textContent = `downloading ${done}/${total}`;
    });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = downloadName;
    document.body.appendChild(a);
    a.click();
    a.remove();
    URL.revokeObjectURL(url);
  } catch (err) {
    alert(`Could not download ${filename}: ${err.message}`);
  } finally {
    button.disabled = false;
    button.textContent = original;
  }
}

function renderDownloadButton(recordId, filename) {
  const btn = document.createElement("button");
  btn.className = "download-btn";
  btn.type = "button";
  btn.title = `Download ${filename.replace(/\.json$/i, "")}.zip`;
  btn.setAttribute("aria-label", btn.title);
  btn.textContent = "⬇";
  btn.addEventListener("click", (e) => {
    e.preventDefault();
    e.stopPropagation();
    downloadPhantomZip(recordId, filename, btn);
  });
  return btn;
}

async function loadRegistry() {
  const container = document.getElementById("registry-list");
  try {
    const [registry, catalog] = await Promise.all([fetchRegistry(), fetchCatalog()]);
    renderRegistry(container, registry, catalog);
  } catch (err) {
    container.innerHTML = `
      <p class="error">
        Could not load the catalog / registry (${err.message}).
        See <a href="${REPO_URL}/blob/main/catalog.json">catalog.json</a> and
        <a href="${REPO_URL}/blob/main/registry.json">registry.json</a> on GitHub.
      </p>`;
  }
}

function renderRegistry(container, registry, catalog) {
  // Only the collections named in catalog.json, in catalog order, each resolved
  // to its immutable registry entry.
  const entries = Object.entries(catalog)
    .map(([label, name]) => {
      const entry = registry[name];
      if (!entry) console.warn(`catalog entry "${label}" -> "${name}" is not in the registry`);
      return [label, name, entry];
    })
    .filter(([, , entry]) => entry);
  if (entries.length === 0) {
    container.innerHTML = `<p class="muted">No entries yet.</p>`;
    return;
  }
  container.innerHTML = "";

  // Collect all unique tags across all entries, sorted
  const allTags = [...new Set(
    entries.flatMap(([, , e]) => Array.isArray(e.keywords) ? e.keywords : [])
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

  for (const [label, name, entry] of entries) {
    const el = renderEntry(label, name, entry);
    const keywords = Array.isArray(entry.keywords) ? entry.keywords : [];
    cards.push({ el, keywords });
    container.appendChild(el);
  }
}

const TISSUE_PROPERTIES = ["T1", "T2", "T2'", "ADC", "dB0", "B1+", "B1-"];
const ARRAY_PROPERTIES = new Set(["B1+", "B1-"]);


function renderEntry(label, name, entry) {
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
      <span class="card-title">${escape(label)}</span>
      ${renderTags(entry.keywords)}
      <span class="card-meta">${phantomCount} phantom${phantomCount === 1 ? "" : "s"}</span>
    </summary>
    <div class="card-body">
      ${entry.description ? `<p class="entry-desc">${escape(entry.description)}</p>` : ""}
      <dl class="entry-fields">
        <dt>Registry name</dt><dd><code>${escape(name)}</code></dd>
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
    phantomSection = renderPhantomSection(phantoms, recordId);
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
function renderPhantomSection(phantoms, recordId) {
  const wrap = document.createElement("div");
  wrap.className = "list-section";
  const tree = renderPhantomTree(phantoms, recordId);
  wrap.appendChild(tree);
  wrap.loadPhantoms = tree.loadPhantoms;
  return wrap;
}

// Renders one level of a (possibly nested) phantoms array: leaf filenames
// become phantom cards (see renderPhantomList); group objects become nested
// accordions (see renderPhantomGroup) that lazily render their own subtree
// on first expand.
function renderPhantomTree(entries, recordId) {
  const container = document.createElement("div");
  container.className = "phantom-tree";

  const files = entries.filter((e) => typeof e === "string");
  const groups = entries.filter((e) => e && typeof e === "object");

  let list = null;
  if (files.length > 0) {
    list = renderPhantomList(files, recordId);
    container.appendChild(list);
  }
  for (const group of groups) {
    container.appendChild(renderPhantomGroup(group, recordId));
  }

  // Only the leaf list has anything to fetch at this level - nested groups
  // load themselves lazily when expanded (see renderPhantomGroup).
  container.loadPhantoms = () => { if (list) list.loadPhantoms(); };

  return container;
}

// A named group of phantom entries, rendered as a nested collapsible card
// that drills into its own subtree. Lazily loads phantom metadata (resolution,
// tissues) for its subtree only the first time it's expanded - important
// since a deep tree can otherwise trigger hundreds of Zenodo fetches on page
// load.
function renderPhantomGroup(group, recordId) {
  const groupPhantoms = Array.isArray(group.phantoms) ? group.phantoms : [];
  const count = countPhantoms(groupPhantoms);

  const details = document.createElement("details");
  details.className = "card group";
  details.innerHTML = `
    <summary class="card-summary">
      <span class="card-title">${escape(group.group)}</span>
      ${group.default ? `<span class="tag group-default-tag">default: ${escape(group.default)}</span>` : ""}
      ${group.default && recordId ? `<span class="download-slot"></span>` : ""}
      <span class="card-meta">${count} phantom${count === 1 ? "" : "s"}</span>
    </summary>
    <div class="card-body">
      ${group.description ? `<p class="entry-desc">${escape(group.description)}</p>` : ""}
      <div class="group-content-slot"></div>
    </div>
  `;

  if (group.default && recordId) {
    details.querySelector(".download-slot").appendChild(renderDownloadButton(recordId, group.default));
  }

  const tree = renderPhantomTree(groupPhantoms, recordId);
  details.querySelector(".group-content-slot").appendChild(tree);

  let loaded = false;
  details.addEventListener("toggle", () => {
    if (!details.open || loaded) return;
    loaded = true;
    tree.loadPhantoms();
  });

  return details;
}

// A leaf-level phantom list: one collapsible card per phantom, styled like a
// group card (filename, tissues, resolution, download button in the summary)
// but expanding to show the tissue table / raw JSON instead of a subtree.
function renderPhantomList(phantoms, recordId) {
  const container = document.createElement("div");
  container.className = "phantom-tree";

  const rows = phantoms.map((filename) => {
    const details = document.createElement("details");
    details.className = "card group phantom";
    details.innerHTML = `
      <summary class="card-summary">
        <span class="card-title"><code>${escape(filename)}</code></span>
        <span class="card-meta phantom-tissues"><span class="loading-text">…</span></span>
        <span class="card-meta phantom-resolution"><span class="loading-text">…</span></span>
        ${recordId ? `<span class="download-slot"></span>` : ""}
      </summary>
      <div class="card-body"></div>
    `;

    if (recordId) {
      details.querySelector(".download-slot").appendChild(renderDownloadButton(recordId, filename));
    }

    container.appendChild(details);

    return {
      filename,
      details,
      body: details.querySelector(".card-body"),
      tissuesEl: details.querySelector(".phantom-tissues"),
      resEl: details.querySelector(".phantom-resolution"),
    };
  });

  container.loadPhantoms = () => {
    for (const { filename, details, body, tissuesEl, resEl } of rows) {
      if (!recordId) {
        const dash = '<span class="muted">—</span>';
        tissuesEl.innerHTML = dash;
        resEl.innerHTML = dash;
        body.innerHTML = `<p class="muted" style="padding:0.5rem 0">Not available.</p>`;
        continue;
      }

      const dataPromise = fetchPhantomJson(recordId, filename);

      dataPromise
        .then((data) => {
          const res = data?.reslice_to?.resolution;
          resEl.textContent = Array.isArray(res) ? res.join("×") : "native";

          const tissueNames = Object.keys(data?.tissues || {});
          tissuesEl.textContent = tissueNames.length > 0 ? tissueNames.join(", ") : "—";
        })
        .catch((err) => {
          const errHtml = `<span class="muted" title="${escape(err.message)}">!</span>`;
          tissuesEl.innerHTML = errHtml;
          resEl.innerHTML = errHtml;
        });

      let bodyLoaded = false;
      details.addEventListener("toggle", () => {
        if (!details.open || bodyLoaded) return;
        bodyLoaded = true;
        body.innerHTML = `<p class="muted" style="padding:0.5rem 0">Loading…</p>`;
        dataPromise
          .then((data) => renderPhantomDetail(body, data))
          .catch((err) => {
            body.innerHTML = `<p class="muted" style="padding:0.5rem 0">Could not load: ${escape(err.message)}</p>`;
          });
      });
    }
  };

  return container;
}

// Renders a phantom's tissue table (with a table/JSON toggle) into `container`.
function renderPhantomDetail(container, rawData) {
  const tissues = rawData?.tissues || {};
  const tissueNames = Object.keys(tissues);

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

  const content = document.createElement("div");
  content.className = "phantom-detail-content";

  function showTable() {
    // `patient` is optional; omitting it means FFS (see ../NIFTI.md).
    const position = rawData?.patient?.position;
    const positionHtml = position
      ? `<p class="muted" style="padding:0.5rem 0 0">patient position: <code>${escape(position)}</code></p>`
      : "";
    content.innerHTML = positionHtml + (tissueNames.length > 0
      ? renderTissueTable(tissues, tissueNames)
      : `<p class="muted" style="padding:0.5rem 0">No tissues defined.</p>`);
    toggleBtn.setAttribute("aria-checked", "false");
    toggleLabel.textContent = "table";
  }

  function showJson() {
    const pre = document.createElement("pre");
    pre.className = "json-viewer";
    pre.innerHTML = highlightJson(rawData);
    content.innerHTML = "";
    content.appendChild(pre);
    toggleBtn.setAttribute("aria-checked", "true");
    toggleLabel.textContent = "json";
  }

  let showingJson = false;
  showTable();

  toggleBtn.addEventListener("click", () => {
    showingJson = !showingJson;
    showingJson ? showJson() : showTable();
  });

  container.innerHTML = "";
  container.appendChild(toggleWrap);
  container.appendChild(content);
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
    fetchRecordTotalSize(recordId)
      .then((total) => {
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

function escape(s) {
  return String(s)
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

loadRegistry();
