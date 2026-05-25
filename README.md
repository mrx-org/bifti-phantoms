# NIfTI Phantoms

A universal, implementation-agnostic format for storing MRI simulation phantoms.
A phantom is one **JSON** file defining tissues and their MR properties (T1, T2,
…), referencing **NIfTI** files for per-voxel data — both widely supported and
easy to view, edit and version. The goal: vary experiments by editing JSON, not
code, so phantom data is exchangeable and reproducible.

## Specification

- [SPEC.md](SPEC.md) — overview and folder layout.
- [JSON.md](JSON.md) — the phantom JSON: structure, units, system, tissues.
- [NIFTI.md](NIFTI.md) — the NIfTI files: format and coordinate conventions.
- [nifti-phantom-v1.schema.json](nifti-phantom-v1.schema.json) — JSON Schema for validation.

## Registry

A shared index of public phantoms anyone can contribute to via a PR; the data is
hosted on [Zenodo](https://zenodo.org/).

- [REGISTRY.md](REGISTRY.md) — how it works and how to contribute.
- [registry.json](registry.json) — the list of phantom collections.
- [nifti-registry.schema.json](nifti-registry.schema.json) — JSON Schema for registry entries.

## Reference implementation

- [demo/](demo/DEMO.md) — a small Python example that loads a phantom into NumPy
  arrays and plots every tissue; a starting point for porting the format.
