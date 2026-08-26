# BIfTI Phantom - File Format Specification

> [!NOTE]
> **Spec version: v1.** The version is the discriminator baked into `$schema`
> (`bifti-phantom-v1`, see [JSON.md](JSON.md#schema)). It bumps only on a
> breaking change to the format; since the schema sets
> `additionalProperties: false` throughout, there is currently no
> backward-compatible way to add fields within a version — any addition
> requires a new version tag.

The specification has two parts:

- **[JSON.md](JSON.md)** - the phantom JSON file: top-level structure, units,
  system parameters, tissues, and how property values reference NIfTI data.
  Validated by [`bifti-phantom-v1.schema.json`](bifti-phantom-v1.schema.json)
  (JSON Schema draft 2020-12).
- **[NIFTI.md](NIFTI.md)** — the per-voxel NIfTI files: format requirements and
  the coordinate-system conventions they must follow.

## Folder structure

The naming of files is a *convention*: implementations can but are not required
to reject non-conforming names. The supported convention is:

```
📂 subj42
├ 📄 subj42.nii.gz
├ 📄 subj42_dB0.nii.gz
├ 📄 subj42_B1+.nii.gz
├ ...
├ 📄 subj42-3T.json
└ 📄 subj42-7T.json
```

The phantom name is `subj42`, used as the directory name and file prefix. It is
available here in two variants: `subj42-3T.json` and `subj42-7T.json`. The
structure of each JSON file is described in [JSON.md](JSON.md).

Per-voxel data is stored in `<name>_<property>.nii(.gz)` files; the `density`
map omits the property postfix. The properties are listed in
[JSON.md](JSON.md#tissue) and the NIfTI format is described in
[NIFTI.md](NIFTI.md). The `density` property is required for every tissue loaded
from NIfTI (as opposed to a constant value).
