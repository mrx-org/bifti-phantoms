# BIfTI Phantom — File Format Specification

The **BIfTI phantom specification** describes the storage of data suitable for
MR *imaging* simulations. A phantom consists of one or more **NIfTI files** for
per-voxel data and a single **JSON file** that defines the phantom and
references the NIfTI files.

The specification has two parts:

- **[JSON.md](JSON.md)** — the phantom JSON file: top-level structure, units,
  system parameters, tissues, and how property values reference NIfTI data.
  Validated by [`nifti-phantom-v1.schema.json`](nifti-phantom-v1.schema.json)
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

## Example

See [`example/subj42-3T.json`](example/subj42-3T.json) for a complete phantom.
