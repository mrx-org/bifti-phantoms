# Add your phantoms to the registry

Three steps, one pull request. You don't need to know anything about the
tooling beyond a text editor and a Zenodo account.

| Step | Where | What you do |
|------|-------|-------------|
| 1. Host | [Zenodo](https://zenodo.org/) | Upload your NIfTI + JSON files as one record and publish it |
| 2. Register | [registry.json](registry.json) | Add **one new entry** pointing at the Zenodo DOI (permanent) |
| 3. Catalog | [catalog.json](catalog.json) | Add **one line** with a display label pointing at that entry (so it shows up in tools and the [browser](https://mrx-org.github.io/bifti-phantoms/)) |

Background on why there are two files: [REGISTRY.md](REGISTRY.md).

## 1. Zenodo

1. Check that every phantom JSON validates and loads (see [SPEC.md](SPEC.md)).
   The JSON files should reference their NIfTIs by **relative filename**.
2. On Zenodo, create a **new upload** and add *all* phantom JSONs and *all* the
   NIfTI files they reference to the **same record**.
   - More than ~100 files? Bundle the JSONs into a single `configs.tar`
     (never mix loose JSONs and `configs.tar`; see [REGISTRY.md](REGISTRY.md#config-archives)).
3. Fill in title, authors, and a license (e.g. `CC-BY-4.0`, `CC0-1.0`), then **Publish**.
4. Copy the **version DOI** (shown as `10.5281/zenodo.<id>`). Use the specific
   version DOI, not the "all versions" concept DOI.

> Published Zenodo files can't be changed. For a fix, publish a new version and
> add a new registry entry (number bumped), see [below](#revising-a-collection).

## 2. `registry.json`

Add one entry at the end of the file. The key has the form
`<author>-<name>-<number>`: lowercase surname (no hyphens), a short slug
(letters, digits, `_`, `.`; no hyphens), and a 3-digit number starting at `001`.

```json
"smith-liver-001": {
  "description": "Liver phantom at 1.5T and 3T, multi-slice.",
  "keywords": [ "liver", "3T", "1.5T", "3d" ],
  "authors": [
    { "name": "Smith, Jane", "orcid": "0000-0000-0000-0000", "affiliation": "Example University" }
  ],
  "license": "CC-BY-4.0",
  "doi": "10.5281/zenodo.1234567",
  "phantoms": [ "liver_1.5T.json", "liver_3T.json" ]
}
```

Required: `description`, `authors`, `license`, `doi`, `phantoms` (JSON filenames
exactly as in the Zenodo record). Optional: `keywords`. Many variants?
[Group them](REGISTRY.md#grouping-phantoms).

**Never edit or remove an existing entry**; CI blocks it.

## 3. `catalog.json`

Add one line: a human-readable label mapped to your new registry key.

```json
"Liver phantom": "smith-liver-001"
```

The order of lines is the display order. This file may be edited freely later
(rename, reorder, re-point, remove).

## 4. Check and open the PR

```sh
pip install jsonschema
python tools/validate_registry.py
python tools/validate_catalog.py
python tools/check_registry_immutable.py   # needs origin/main fetched
```

Then open a PR. The template will walk you through the checklist.

## Revising a collection

Published entries are frozen. To publish a fixed or extended dataset: upload a
new Zenodo version, add a **new** `registry.json` entry with the number bumped
(`smith-liver-001` → `smith-liver-002`), and re-point the existing label in
`catalog.json` at it. The old entry stays forever so old references keep working.
