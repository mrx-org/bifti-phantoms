# BIfTI Phantoms

*Bloch Informatics Technology Initiative* phantom format - a playful riff on **NIfTI**, but specific to MRI simulation data.

A universal, implementation-agnostic format for storing MRI simulation phantoms.
A phantom is one **JSON** file defining tissues and their MR properties, referencing **NIfTI** files for per-voxel data.

> [!IMPORTANT]
> **Goals**
> - _easy to use:_ Human readable configs, existing viewers for 3D tissue data
> - _easy to share:_ Strict spec ensures consistent phantom structuring
> - _easy to extend:_ General approach supports future extensions
> - _easy to implement:_ JSON and NIfTI are widely supported

Usage from python:
```python
# Example will follow - demo is under construction.
```

## Specification

- [SPEC.md](SPEC.md) - overview and folder layout.
- [JSON.md](JSON.md) - the phantom JSON: structure, units, system, tissues.
- [NIFTI.md](NIFTI.md) - the NIfTI files: format and coordinate conventions.
- [REGISTRY.md](REGISTRY.md) - the accompanying registry for phantom sharing.

## Registry

Example phantoms are available in the public registry: [registry.json](registry.json).
The registry can also be viewed here: https://mrx-org.github.io/bifti-phantoms/

This registry exists for the purpose of making sharing easy and experiments reproducible. Anyone is welcome to add new phantoms to the registry. Phantom files themselves can be hosted for free on [Zenodo](https://zenodo.org/), under any appropriate license and attribution. The registry is a central place to collect those phantoms - add to it with a pull request that extends [registry.json](registry.json) with new entries.

## Reference implementation

```bash
# Load bifti phantoms from rust
cargo add --git https://github.com/mrx-org/bifti-phantoms bifti
# Load bifti phantoms from Python
pip install "git+https://github.com/mrx-org/bifti-phantoms.git#subdirectory=python/bifti"
# Using the uv package manager:
uv add --git https://github.com/mrx-org/bifti-phantoms --subdirectory python/bifti bifti
```

[demo/](demo/DEMO.md) — a small Python example that loads a phantom into NumPy
arrays and plots every tissue; a starting point for porting the format. Run
`python demo.py` (no args) to list the registry's phantoms and download + plot
a chosen one. WIP: code and documentation will be improved in the future.
