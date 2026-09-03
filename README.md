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

> [!NOTE]
> **Status:** the phantom spec is **v1** (see [SPEC.md](SPEC.md)); the
> [registry](#registry) format is **v1** (see [REGISTRY.md](REGISTRY.md))

## Quick example

A minimal phantom is a JSON sidecar next to its NIfTI(s) — one tissue,
`density` from a NIfTI file, everything else a plain number:

```json
{
  "$schema": "https://raw.githubusercontent.com/mrx-org/bifti-phantoms/refs/heads/main/bifti-phantom-v1.schema.json",
  "units":  { "gyro": "MHz/T", "B0": "T", "T1": "s", "T2": "s", "T2'": "s", "ADC": "10^-3 mm^2/s", "dB0": "Hz", "B1+": "rel", "B1-": "rel" },
  "system": { "gyro": 42.5764, "B0": 3.0 },
  "tissues": {
    "gm": { "density": "subj42.nii.gz[0]", "T1": 1.56, "T2": 0.083 }
  }
}
```

Loading it — from Python:

```python
from bifti import NumpyPhantom

phantom = NumpyPhantom.load("subj42.json")
tissue = phantom.tissues["gm"]  # a NumpyTissue: density, T1, T2, ... as np.ndarray
print(tissue.shape, tissue.T1.mean())
```

...or from Rust:

```rust
use bifti::{Phantom, VolumeData};

let phantom = Phantom::load("subj42.json")?;
let tissue = &phantom.tissues["gm"];
if let VolumeData::Float64(t1) = &tissue.t1.data {
    println!("{:?} {}", tissue.density.shape, t1.iter().sum::<f64>() / t1.len() as f64);
}
```

A property (`T1`, `dB0`, `B1+`, ...) can also be a reference to a NIfTI
sub-volume, or a NIfTI reference with a per-voxel expression applied — see
[JSON.md](JSON.md) for the full picture, or the worked examples in
[python/bifti/examples/data/](python/bifti/examples/data/).

## What's in this repo

| Path | Purpose |
|------|---------|
| [SPEC.md](SPEC.md) | Overview and folder layout. |
| [JSON.md](JSON.md) | The phantom JSON: structure, units, system, tissues. |
| [NIFTI.md](NIFTI.md) | The NIfTI files: format, coordinate conventions and patient position. |
| [REGISTRY.md](REGISTRY.md) | The registry: how phantoms are hosted and shared. |
| [bifti-phantom-v1.schema.json](bifti-phantom-v1.schema.json) / [bifti-registry.schema.json](bifti-registry.schema.json) / [bifti-catalog.schema.json](bifti-catalog.schema.json) | JSON Schemas validating a phantom JSON / [registry.json](registry.json) / [catalog.json](catalog.json). |
| [registry.json](registry.json) | Immutable archive of every published collection — see [Registry](#registry). |
| [catalog.json](catalog.json) | Living discovery list: which collections tools show, mapped to registry names. |
| [python/bifti/](python/bifti/) | Installable Python package + examples. |
| [rust/bifti/](rust/bifti/) | Installable Rust crate + examples. |
| [docs/](docs/) | Source of the registry browser at https://mrx-org.github.io/bifti-phantoms/. |
| [tools/](tools/) | CI scripts: phantom/registry schema validation, immutability checks. |

> [!IMPORTANT]
> The example implementations for Python and Rust were built with the help of
> LLMs and not yet reviewed thouroughly. They might contain bugs and currently
> not live up to the targeted quality standard. This will change in the future.

## Registry

Example phantoms are available in the public registry. Every published
collection has a permanent entry in [registry.json](registry.json) (the
immutable archive); [catalog.json](catalog.json) is the curated, freely-editable
list of which of those collections tools surface, each mapped to its immutable
registry name. Browse the catalog here: https://mrx-org.github.io/bifti-phantoms/

This exists to make sharing easy and experiments reproducible. Anyone is welcome
to add new phantoms. Phantom files themselves can be hosted for free on
[Zenodo](https://zenodo.org/), under any appropriate license and attribution.
Add one with a pull request that adds an entry to [registry.json](registry.json)
and a label pointing at it in [catalog.json](catalog.json). See
[REGISTRY.md](REGISTRY.md) for the full contribution workflow.

## Reference implementation

```bash
# Load bifti phantoms from rust
cargo add --git https://github.com/mrx-org/bifti-phantoms bifti
# Load bifti phantoms from Python
pip install "git+https://github.com/mrx-org/bifti-phantoms.git#subdirectory=python/bifti"
# Using the uv package manager:
uv add --git https://github.com/mrx-org/bifti-phantoms --subdirectory python/bifti bifti
```

For more information, including runnable examples and each package's full
API, read the README of the [Python `bifti` package](python/bifti/README.md)
or [Rust `bifti` crate](rust/bifti/README.md).

### Python vs Rust

The two implementations currently have some discrepancies:

| | Python (`python/bifti`) | Rust (`rust/bifti`) |
|---|---|---|
| Loaded representation | `NumpyPhantom.tissues: dict[str, NumpyTissue]` — NumPy arrays | `Phantom.tissues: HashMap<String, Tissue>` - `Volume`s (affine + shape + `VolumeData`) |
| Complex-valued NIfTI data (e.g. complex `B1+`/`B1-`) | **Silently drops the imaginary part:** `nibabel`'s data is cast with `np.asarray(..., dtype=np.float64)` | Fails with `Error::UnsupportedDataType` |
| Reslicing (`reslice_to`) | Shared approach: density-weighted footprint averaging (see below). Uses `torch` when installed, NumPy otherwise | Same approach, own implementation; all NIfTI data types including complex |
| Catalog / registry access | `load_catalog()`, `load_registry()`, `load_registry_phantom(collection, name)` | `Catalog::load()`, `Registry::load()`, `registry.load_registry_phantom(collection, name, cache_dir)` |
| Unknown fields | Warned about at every level (phantom, `system`, `patient`, `reslice_to`, tissue, transformed reference), then dropped | Warned about for the phantom and its tissues; kept in `unknown` so `save` round-trips them |
| Examples | 4 runnable scripts: plotting, KomaMRI export, MR-zero simulation, legacy-phantom conversion (see [python/bifti/README.md](python/bifti/README.md#examples)) | 1 runnable example: random registry download with `tracing` instrumentation (see [rust/bifti/README.md](rust/bifti/README.md#examples)) |
| Optional instrumentation | - | `tracing` feature (spans for downloading, NIfTI loading, `func` evaluation) |

### Reslicing

Both implementations resample the same way. Each output voxel is averaged over the
whole source region it covers, rather than interpolated from the few voxels nearest
its centre — so resampling onto a coarser grid actually averages instead of throwing
most of the data away.

The averaging is **weighted by the tissue's `density`**. Outside the source FOV, and
in the background between tissues, every map reads `0`, and `T1`/`T2`/`T2'`/`ADC`/
`dB0`/`B1±` are intensive quantities: averaging them against those zeros would pull
them towards zero at every edge. Weighting by density excludes the empty voxels
instead. `density` itself is extensive, so it keeps a plain footprint average and
correctly falls off where an output voxel is only partly filled.

For axis-aligned grids the average is computed exactly, as a true box average.
Oblique transforms fall back to quadrature over the output voxel's parallelepiped,
and axes that are not being downsampled keep plain linear interpolation.
