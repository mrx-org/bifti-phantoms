# NIfTI Phantom Registry

[`registry.json`](registry.json) is a public, PR-editable index of NIfTI
phantoms. It only **references** data — the phantoms themselves are hosted by a
provider like [Zenodo](https://zenodo.org/). Anyone can publish a phantom and add
it via a pull request. Entries are validated against
[`nifti-registry.schema.json`](nifti-registry.schema.json) (JSON Schema
draft 2020-12). The format carries no version tag and allows extra properties, so
it can be migrated in place if it ever needs to change.

## Hosting model

A phantom consists of large, static **NIfTI** files and a small **JSON** file
that is tweaked more often (new field strengths, revised T1/T2, …). Both are kept
in **one Zenodo record**, which gives us exactly the two identifiers we need:

- a **concept DOI** that always resolves to the record's *latest* version, and
- a **version DOI** per published version that is *immutable* — two people citing
  the same version DOI are guaranteed byte-identical files. The version DOI **is**
  the integrity guarantee, so the registry stores no checksums.

You iterate freely (locally / in a fork), then *freeze* by publishing a Zenodo
version. Zenodo's **"New version"** flow carries unchanged files forward without
re-uploading them, so revising the JSON does not re-upload the NIfTI.

A record may bundle a **set** of related phantoms (e.g. one subject across field
strengths, or a whole cohort). Record granularity is independent of registry
granularity: the registry still lists each phantom individually, and several
entries may share the same `source`.

## Entry format

`registry.json` is an object with a `phantoms` array. Each entry:

| Field | Req. | Description |
|-------|------|-------------|
| `id` | yes | Unique key, conventionally the phantom name (`subj42`). |
| `title` | yes | Human-readable name. |
| `description` | yes | One or two sentences. |
| `authors` | yes | List of `{ name, orcid?, affiliation? }`. |
| `license` | yes | SPDX id, e.g. `CC-BY-4.0`, `CC0-1.0`. |
| `source` | yes | Hosting record (see below). |
| `variants` | yes | The JSON configs in the record (≥ 1). |
| `keywords` | no | Discovery tags (`brain`, `synthetic`, `3d`, …). |
| `collection` | no | Label grouping phantoms that share a record. |

`source` — where the data lives:

| Field | Req. | Description |
|-------|------|-------------|
| `provider` | yes | `zenodo` (only provider defined in v1). |
| `concept_doi` | yes | Record concept DOI → always the latest version. |
| `doi` | yes | Version DOI this entry is **pinned** to (reproducibility). |
| `url` | no | Convenience link, e.g. `https://doi.org/<doi>`. |

`variants[]` — one per phantom JSON in the record:

| Field | Req. | Description |
|-------|------|-------------|
| `name` | yes | Variant name (`subj42-3T`). |
| `file` | yes | JSON filename inside the record. |
| `B0` | no | Main field strength [T], for discovery. |

Resolving a phantom: take `source` → the record version (`doi`) → its file base,
then download `variants[].file`; the NIfTI files it references resolve from the
same record. Use `concept_doi` instead to follow the latest version.

Deliberately minimal: tissue lists, resolution and channel counts are discovered
by opening the phantom JSON, not duplicated here.

## Contributing a phantom

1. Assemble the phantom set (NIfTI + JSON) following the layout in
   [SPEC.md](SPEC.md).
2. Upload **all files** to a single Zenodo record and publish it. To revise the
   JSON later, use Zenodo **"New version"** and keep the existing NIfTI files.
3. Open a PR adding one entry per phantom to [`registry.json`](registry.json),
   pinning `source.doi` to the published version DOI (and recording its
   `concept_doi`).

A GitHub Action validates `registry.json` against the schema on every PR. Run the
same check locally before opening one:

```sh
pip install jsonschema
python tools/validate_registry.py
```
