# BIfTI Phantom Registry

> [!NOTE]
> **Registry status: alpha.** Unlike the phantom spec (see [SPEC.md](SPEC.md)),
> this format carries no version tag of its own, so it may still change shape.
> Registry entries stay immutable once merged either way.

Public phantom sharing is split across two files at the repo root:

| File | Role | Mutable? |
|------|------|----------|
| [`registry.json`](registry.json) | **Immutable archive.** Every phantom collection ever published, each keyed by a permanent name. | No — entries frozen once merged. |
| [`catalog.json`](catalog.json) | **Living discovery list.** Maps a human-readable label to a registry name. Everything a tool shows when you ask it to "list phantoms" comes from here. | Yes — add / update / remove entries freely. |

The registry only **references** data — the phantoms are hosted on
[Zenodo](https://zenodo.org/) — and anyone can add one via a pull request.
`registry.json` is validated against
[`bifti-registry.schema.json`](bifti-registry.schema.json), `catalog.json`
against [`bifti-catalog.schema.json`](bifti-catalog.schema.json).

## Why two files

An addressable name must resolve to the same bytes forever, or references stop
being reproducible. But a flat, append-only list is also the thing every tool
enumerates — and it fills up with superseded, broken, or uninteresting entries
that you can never take out of view.

So the two jobs are separated. `registry.json` keeps a permanent entry for every
collection. `catalog.json` decides which of those collections are worth showing;
it can be reordered, pruned, and repointed at will. A tool lists the catalog,
then resolves each label to its immutable `registry.json` name — the name you use
to address that collection from then on.

## Hosting model

A phantom is large, static **NIfTI** files plus a small **JSON** file that
describes it. Both live in **one Zenodo record**. Each published version of that
record gets an immutable **version DOI** — the same DOI always resolves to
byte-identical files, so it *is* the integrity guarantee and the registry stores
no checksums.

A Zenodo version DOI is `10.5281/zenodo.<record_id>`: it embeds the host and the
record id, so the `doi` alone is enough to download the files. The registry
therefore stores no `provider` or `url` — both are implied. (Zenodo is the only
host for now; another could be added in place if needed.)

Each entry (keyed by collection name) maps to exactly one record, listing every
phantom JSON in it. A record's phantoms are never split across entries, and no
two entries share a `doi`.

## `registry.json` — the immutable archive

### Collection names

Every entry key has the fixed form **`<author>-<name>-<number>`**, matching
`^[a-z]+-[A-Za-z0-9_.]+-[0-9]{3}$`:

| Part | Rule | Examples |
|------|------|----------|
| `author` | lowercase surname, no hyphens | `endres`, `duarte`, `zaiss` |
| `name` | short slug; letters, digits, `_`, `.`; no hyphens | `breast`, `brainweb_highres`, `brainweb_subj04_3T_0.5mm` |
| `number` | zero-padded 3 digits, starts at `001` | `001`, `002` |

```
endres-brainweb-001
magda-breast-001
zaiss-brainweb_subj04_3T_0.5mm-001
```

### Entry immutability

Once an entry is merged into `main` it is **frozen**: its key, its `doi`, its
`phantoms` list, and every other field must not be changed, removed, or renamed.
[`tools/check_registry_immutable.py`](tools/check_registry_immutable.py) enforces
this on every pull request.

To publish a **revised or extended** dataset, open a PR that **adds a new entry**
with the number bumped:

```
endres-brainweb-001   →   endres-brainweb-002
```

The old entry stays in `registry.json` forever, so any existing reference to it
keeps resolving. Whether users *see* the old or the new one is a separate
decision — that's what `catalog.json` is for (repoint the label at
`endres-brainweb-002`).

### Reproducibility

A `registry.json` name on `main` always resolves to the same `doi` and therefore
the same byte-identical files. To reference a specific phantom unambiguously use
`<collection>/<file>` — e.g. `endres-brainweb-001/subj04-3T-1mm.json`. To
additionally pin against future new collections, record the git commit:
`a1b2c3d endres-brainweb-001/subj04-3T-1mm.json`.

A **catalog label is not a stable reference** — it can be re-pointed or removed.
Always resolve it to the `registry.json` name (and ideally `<collection>/<file>`)
before recording a reference.

### Entry format

`registry.json` is a top-level object mapping each collection name to its entry:

```json
{
  "duarte-breast-001": { "description": "…", "doi": "10.5281/zenodo.<id>", "phantoms": [ "breast_3T.json" ] }
}
```

Each entry value has these fields:

| Field | Req. | Description |
|-------|------|-------------|
| `description` | yes | One or two sentences describing the collection. |
| `authors` | yes | List of `{ name, orcid?, email?, affiliation? }`. |
| `license` | yes | SPDX id, e.g. `CC-BY-4.0`, `CC0-1.0`. |
| `doi` | yes | Immutable Zenodo version DOI (`10.5281/zenodo.<id>`). |
| `phantoms` | yes | JSON filenames in the record (≥ 1), e.g. `breast_3T.json`. |
| `keywords` | no | Discovery tags (`brain`, `synthetic`, `3d`, …). |

Each `phantoms[]` entry is referenced as `<collection>/<filename>` and pulls in
the NIfTI files it needs from the same record; names should be self-describing
(field strength, options) since there is no per-phantom description. Tissue
lists, resolution and channel counts are not duplicated here — open the phantom
JSON for those.

### Grouping phantoms

A `phantoms[]` entry is normally a plain filename string, but for collections
with many configuration axes (field strength, resolution, orientation, …) a
flat list quickly becomes unreadable. An entry may instead be a **group**
object that nests further entries:

```json
{
  "group": "3T",
  "description": "Properties for 3T main field strength",
  "default": "subj04-3T-05mm.json",
  "phantoms": [
    "subj04-3T-05mm.json", "subj05-3T-05mm.json",
    { "group": "coronal", "phantoms": [ "subj04-3T-05mm-cor.json", "subj05-3T-05mm-cor.json" ] }
  ]
}
```

Filenames and groups can be freely mixed within the same `phantoms[]` array,
and groups nest to any depth. `description` and `default` (a representative
filename from somewhere inside the group, for callers that just want one
example) are both optional; only `group` and `phantoms` are required. A group
is purely organizational — it carries no file of its own, so addressing is
unaffected: every filename, no matter how deeply nested, is still referenced
as `<collection>/<filename>`. This is fully backwards compatible — an entry
that never uses groups is just a flat array of strings, as before.

## `catalog.json` — the discovery list

A top-level object mapping a **human-readable label** to a **registry collection
name**:

```json
{
  "Bifti demo phantoms": "endres-bifti_demo-001",
  "Breast phantom": "magda-breast-001",
  "Brainweb collection (1mm source data)": "endres-brainweb-001"
}
```

Rules:

- The **key is a free-text label**, shown by tools as the collection's title.
  Labels are unique (object keys are). Object key **order is display order**.
- The **value must be a current key in `registry.json`**.
  [`tools/validate_catalog.py`](tools/validate_catalog.py) enforces this.
- **Add, update (re-point), reorder, and remove entries freely.** There is no
  immutability check on this file. Removing an entry hides a collection from
  tools; the `registry.json` entry (and any reference to it) keeps working.

## Contributing a collection

1. Assemble the phantom set (NIfTI + JSON) following [SPEC.md](SPEC.md).
2. Upload **all files** to a single Zenodo record and publish.
3. Open one PR that:
   - **adds one new entry** to [`registry.json`](registry.json): pick an
     `<author>-<name>-<number>` name (bump the number if you are revising an
     existing collection), list every phantom JSON under `phantoms`, and set
     `doi` to the published version DOI;
   - **adds or updates the matching entry** in [`catalog.json`](catalog.json):
     a label pointing at that new name (for a revision, re-point the existing
     label).

**Never edit or remove an existing `registry.json` entry.** A CI check blocks
any PR that modifies an already-merged collection. Editing `catalog.json` freely,
on the other hand, is expected.

Run the checks locally before opening a PR:

```sh
pip install jsonschema
python tools/validate_registry.py        # registry schema validation
python tools/validate_catalog.py         # catalog schema + resolution
python tools/check_registry_immutable.py # registry immutability (needs origin/main)
```

## Downloading data

The `doi` from the resolved `registry.json` entry is all you need: parse the
Zenodo record id from it (`re.search(r"zenodo\.(\d+)$", doi)`) and pull each file
from `https://zenodo.org/api/records/<record_id>/files/<filename>/content`.

[`python/bifti/src/bifti/registry.py`](python/bifti/src/bifti/registry.py) and
[`rust/bifti/src/registry.rs`](rust/bifti/src/registry.rs) are the reference
implementations: `load_catalog()` / `Catalog::load()` fetch `catalog.json`,
`load_registry()` / `Registry::load()` fetch `registry.json`, and
`load_registry_phantom(collection, name)` (taking an immutable registry name)
downloads a phantom's JSON plus every NIfTI it references into a local cache,
ready to load.

## Config archives

Zenodo records are limited to 100 files. When a collection contains many JSON
config variants (different field strengths, resolutions, slice positions, …), the
configs can be bundled into a single uncompressed TAR file named **`configs.tar`**
stored at the root of the same Zenodo record. NIfTI files are always uploaded
individually — only JSON configs go in the archive.

Archive layout: JSON files are stored **flat** (no subdirectory nesting).

```
configs.tar
├── subj04-3T.json
├── subj04-7T.json
├── subj04-2D-3T.json
…
```

### Lookup order

Every loader resolves a phantom JSON in exactly this order, stopping at the first
success:

| Step | URL |
|------|-----|
| 1. Archive | `…/files/configs.tar/content` → extract `<filename>` |
| 2. Direct | `…/files/<filename>/content` |

### Convention

> **A record MUST contain either only direct JSON files or a single
> `configs.tar` — not a mix of both.** Loaders implement the two-step
> fallback for robustness; users must never rely on it to paper over a
> mixed layout.
