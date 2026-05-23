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
in **one Zenodo record**. Each published version of that record gets an immutable
**version DOI**: two people citing the same version DOI are guaranteed
byte-identical files. The version DOI **is** the integrity guarantee, so the
registry stores no checksums.

Every entry pins `source.doi` to one such immutable version. There is
deliberately **no concept DOI** — following the latest version is the registry's
job, not the record's. The entry is **mutable**: when a phantom is revised (a new
Zenodo version is published) the entry is updated in place to the new version DOI
via a PR. So the registry always points at current data, while each `doi` it
points at is itself frozen.

You iterate freely (locally / in a fork), then *freeze* by publishing a Zenodo
version. Zenodo's **"New version"** flow carries unchanged files forward without
re-uploading them, so revising the JSON does not re-upload the NIfTI.

A record may bundle a **set** of related phantoms (e.g. one subject across field
strengths, or a whole cohort). Record granularity is independent of registry
granularity: the registry still lists each phantom individually, and several
entries may share the same `source`.

### Reproducibility

Because entries are mutable, the immutable unit is the **registry itself**, which
is versioned by git. To pin a phantom in your code, reference its `id` together
with the **nifti-phantoms commit hash** of the registry you resolved it against —
that commit fixes the exact `source.doi`, and therefore the exact files, forever.
Resolve against the registry's `main` HEAD for the newest data; pin to a commit
for a reproducible resolution. There is no need to record DOIs in your code: the
`id` + commit is enough, and it works uniformly across providers.

## Entry format

`registry.json` is an object with a `phantoms` array. Each entry:

| Field | Req. | Description |
|-------|------|-------------|
| `id` | yes | Unique key, conventionally the phantom name (`subj42`). |
| `description` | yes | One or two sentences. |
| `authors` | yes | List of `{ name, orcid?, email?, affiliation? }`. |
| `license` | yes | SPDX id, e.g. `CC-BY-4.0`, `CC0-1.0`. |
| `source` | yes | Hosting record (see below). |
| `variants` | yes | The JSON configs in the record (≥ 1). |
| `keywords` | no | Discovery tags (`brain`, `synthetic`, `3d`, …). |
| `collection` | no | Label grouping phantoms that share a record. |

`source` — where the data lives:

| Field | Req. | Description |
|-------|------|-------------|
| `provider` | yes | `zenodo` (only provider defined in v1). |
| `doi` | yes | Immutable **version DOI** the entry currently points to. Updated in place (new PR) when the data is revised. |
| `url` | no | Convenience link, e.g. `https://doi.org/<doi>`. |

`variants[]` — one per phantom JSON in the record:

| Field | Req. | Description |
|-------|------|-------------|
| `name` | yes | Variant name (`subj42-3T`). |
| `file` | yes | JSON filename inside the record. |
| `B0` | no | Main field strength [T], for discovery. |

Resolving a phantom: take `source` → the record version (`doi`) → its file base,
then download `variants[].file`; the NIfTI files it references resolve from the
same record. To follow the latest data, resolve the entry from the registry's
`main` HEAD; to reproduce an exact resolution, resolve it at a pinned commit.

Deliberately minimal: tissue lists, resolution and channel counts are discovered
by opening the phantom JSON, not duplicated here.

## Contributing a phantom

1. Assemble the phantom set (NIfTI + JSON) following the layout in
   [SPEC.md](SPEC.md).
2. Upload **all files** to a single Zenodo record and publish it. To revise the
   JSON later, use Zenodo **"New version"** and keep the existing NIfTI files.
3. Open a PR adding one entry per phantom to [`registry.json`](registry.json),
   setting `source.doi` to the published version DOI. To revise a phantom later,
   publish a new Zenodo version and open a PR that updates its `source.doi`.

A GitHub Action validates `registry.json` against the schema on every PR. Run the
same check locally before opening one:

```sh
pip install jsonschema
python tools/validate_registry.py
```
