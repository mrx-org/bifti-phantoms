# NIfTI Phantom Registry

[`registry.json`](registry.json) is a public, PR-editable index of NIfTI
phantoms. It only **references** data — the phantoms themselves are hosted on
[Zenodo](https://zenodo.org/). Anyone can publish a phantom and add
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

Every entry pins its `doi` to one such immutable version. There is
deliberately **no concept DOI** — following the latest version is the registry's
job, not the record's. The entry is **mutable**: when a phantom is revised (a new
Zenodo version is published) the entry is updated in place to the new version DOI
via a PR. So the registry always points at current data, while each `doi` it
points at is itself frozen.

A Zenodo version DOI is `10.5281/zenodo.<record_id>` — it embeds both the host
and the record id, so the `doi` alone is enough to resolve and download the
files. The registry therefore stores no `provider` or `url`: both are implied by
the DOI. (Zenodo is the only host for now; another could be added later in place
if requested.)

You iterate freely (locally / in a fork), then *freeze* by publishing a Zenodo
version. Zenodo's **"New version"** flow carries unchanged files forward without
re-uploading them, so revising the JSON does not re-upload the NIfTI.

A record may bundle a **set** of related phantoms (e.g. one subject across field
strengths, or a whole cohort). Record granularity is independent of registry
granularity: the registry still lists each phantom individually, and several
entries may share the same `doi`.

### Reproducibility

Because entries are mutable, the immutable unit is the **registry itself**, which
is versioned by git. To pin a phantom in your code, reference its `id` together
with the **nifti-phantoms commit hash** of the registry you resolved it against —
that commit fixes the exact `doi`, and therefore the exact files, forever.
Resolve against the registry's `main` HEAD for the newest data; pin to a commit
for a reproducible resolution. There is no need to record the DOI in your code:
the `id` + commit is enough.

## Entry format

`registry.json` is an object with a `phantoms` array. Each entry:

| Field | Req. | Description |
|-------|------|-------------|
| `id` | yes | Unique key, conventionally the phantom name (`subj42`). |
| `description` | yes | One or two sentences. |
| `authors` | yes | List of `{ name, orcid?, email?, affiliation? }`. |
| `license` | yes | SPDX id, e.g. `CC-BY-4.0`, `CC0-1.0`. |
| `doi` | yes | Immutable Zenodo **version DOI** (`10.5281/zenodo.<id>`) the entry points to. Updated in place (new PR) when the data is revised. |
| `variants` | yes | The JSON configs in the record (≥ 1). |
| `keywords` | no | Discovery tags (`brain`, `synthetic`, `3d`, …). |
| `collection` | no | Label grouping phantoms that share a record. |

`variants[]` — one per phantom JSON in the record:

| Field | Req. | Description |
|-------|------|-------------|
| `name` | yes | Variant name (`subj42-3T`). |
| `file` | yes | JSON filename inside the record. |
| `B0` | no | Main field strength [T], for discovery. |

Resolving a phantom: read the Zenodo record id from `doi` (the digits in
`…/zenodo.<id>`), then fetch files from the record over the Zenodo API — download
`variants[].file` and the NIfTI files it references (all live in the same
record). To follow the latest data, resolve the entry from the registry's `main`
HEAD; to reproduce an exact resolution, resolve it at a pinned commit.

Deliberately minimal: tissue lists, resolution and channel counts are discovered
by opening the phantom JSON, not duplicated here.

## Contributing a phantom

1. Assemble the phantom set (NIfTI + JSON) following the layout in
   [SPEC.md](SPEC.md).
2. Upload **all files** to a single Zenodo record and publish it. To revise the
   JSON later, use Zenodo **"New version"** and keep the existing NIfTI files.
3. Open a PR adding one entry per phantom to [`registry.json`](registry.json),
   setting `doi` to the published version DOI. To revise a phantom later,
   publish a new Zenodo version and open a PR that updates its `doi`.

A GitHub Action validates `registry.json` against the schema on every PR. Run the
same check locally before opening one:

```sh
pip install jsonschema
python tools/validate_registry.py
```
