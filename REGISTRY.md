# BIfTI Phantom Registry

[`registry.json`](registry.json) is a public, PR-editable index of BIfTI
phantoms. It only **references** data — the phantoms are hosted on
[Zenodo](https://zenodo.org/), and anyone can add one via a pull request. Entries
are validated against [`bifti-registry.schema.json`](bifti-registry.schema.json).
The format carries no version tag and entries allow extra properties, so it can
be migrated in place if it ever needs to change.

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

### Entry immutability

Once an entry is merged into `main` it is **frozen**: its `doi`, `phantoms`
list, and all other fields must not be changed or removed. A CI check enforces
this on every pull request.

To publish a revised or extended dataset, open a PR that **adds a new entry**
with a distinct name. Append a version, date, or descriptor — the naming scheme
is flexible:

```
brainweb-20-v2
brainweb-20-7T
brainweb-20-2025-06
```

The old entry stays in the registry forever so that any existing reference to it
continues to resolve.

### Reproducibility

Because entries are immutable, a collection name on `main` always resolves to
the same `doi` and therefore the same byte-identical files. To reference a
specific phantom unambiguously use `<collection>/<file>` — e.g.
`brainweb-20/subj04.json`. To additionally pin against future new collections,
record the git commit: `a1b2c3d brainweb-20/subj04.json`.

## Entry format

`registry.json` is a top-level object mapping each collection name to its entry:

```json
{
  "mrx-brain-cohort": { "description": "…", "doi": "10.5281/zenodo.<id>", "phantoms": [ "subj42-3T.json" ] }
}
```

The **object key is the collection name**: unique (object keys are), matching
`^[A-Za-z0-9][A-Za-z0-9_.-]*$`, and it namespaces the entry's files in references
(`<collection>/<file>`). Each entry value has these fields:

| Field | Req. | Description |
|-------|------|-------------|
| `description` | yes | One or two sentences describing the collection. |
| `authors` | yes | List of `{ name, orcid?, email?, affiliation? }`. |
| `license` | yes | SPDX id, e.g. `CC-BY-4.0`, `CC0-1.0`. |
| `doi` | yes | Immutable Zenodo version DOI (`10.5281/zenodo.<id>`). |
| `phantoms` | yes | JSON filenames in the record (≥ 1), e.g. `subj42-3T.json`. |
| `keywords` | no | Discovery tags (`brain`, `synthetic`, `3d`, …). |

Each `phantoms[]` entry is referenced as `<collection>/<filename>` and pulls in
the NIfTI files it needs from the same record; names should be self-describing
(field strength, options) since there is no per-phantom description. Tissue
lists, resolution and channel counts are not duplicated here — open the phantom
JSON for those.

## Contributing a collection

1. Assemble the phantom set (NIfTI + JSON) following [SPEC.md](SPEC.md).
2. Upload **all files** to a single Zenodo record and publish.
3. Open a PR that **adds one new entry** to [`registry.json`](registry.json):
   choose a unique collection name, list every phantom JSON under `phantoms`,
   and set `doi` to the published version DOI.

**Never edit or remove an existing entry.** A CI check blocks any PR that
modifies an already-merged collection. To publish a revised dataset, add a new
entry with a new name (e.g. `brainweb-20-v2`).

Run both checks locally before opening a PR:

```sh
pip install jsonschema
python tools/validate_registry.py        # schema validation
python tools/check_registry_immutable.py # immutability (needs origin/main)
```

## Downloading data

The `doi` is all you need: parse the Zenodo record id from it
(`re.search(r"zenodo\.(\d+)$", doi)`) and pull each file from
`https://zenodo.org/api/records/<record_id>/files/<filename>/content`.

[`demo/nifti_registry.py`](demo/nifti_registry.py) is a small reference
implementation: `available_phantoms()` fetches and caches this registry, and
`download_phantom(collection, name)` downloads a phantom's JSON plus every NIfTI
it references into a local cache, ready to load.

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
