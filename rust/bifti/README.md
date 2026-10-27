# bifti

A Rust crate for the [BIfTI phantom format](https://github.com/mrx-org/bifti-phantoms/blob/main/SPEC.md): parse/serialize the
phantom JSON, load a phantom into plain arrays, and fetch phantoms from the
public [registry](https://github.com/mrx-org/bifti-phantoms/blob/main/REGISTRY.md).

```rust
use bifti::{Phantom, VolumeData};

let phantom = Phantom::load("subj42-3T.json")?;
let tissue = &phantom.tissues["gm"];

if let VolumeData::Float64(t1) = &tissue.t1.data {
    let mean = t1.iter().sum::<f64>() / t1.len() as f64;
    println!("gm: mean T1 = {mean:.3} s");
}
```

`phantom.config` is the parsed `BiftiPhantom` (the raw JSON structure: units,
system, tissue definitions) — see [`phantom.rs`](https://github.com/mrx-org/bifti-phantoms/blob/main/rust/bifti/src/phantom.rs) for the full
data model and [`loader.rs`](https://github.com/mrx-org/bifti-phantoms/blob/main/rust/bifti/src/loader.rs) for how each property resolves to
a `Volume`.

## API at a glance

| Item | What it is |
|------|------------|
| `Phantom::load(path)` | Load a phantom JSON + its NIfTIs into `Phantom { config, tissues }`. |
| `Tissue` | One tissue as `Volume`s: `density`, `t1`, `t2`, `t2dash`, `adc`, `db0`, `b1_tx: Vec<Volume>`, `b1_rx: Vec<Volume>`. |
| `Volume` | `{ affine: [[f64; 4]; 3], shape: [usize; 3], data: VolumeData }`. |
| `VolumeData` | `Float32(Vec<f32>)` / `Float64(Vec<f64>)` of the volume's voxels, row-major (`x*ny*nz + y*nz + z`). |
| `phantom.save(dir, json_name)` | Save a loaded phantom as a self-contained folder (NIfTIs + JSON, no `reslice_to`, no `func`); see [Saving](https://github.com/mrx-org/bifti-phantoms/blob/main/README.md#saving). Returns the JSON path. |
| `Volume::save_nifti(path)` / `Volume::save_nifti_stack(volumes, path)` | Write one volume, or several stacked along the 4th dimension, as a deterministic NIfTI-1 file. |
| `BiftiPhantom::load(path)` / `.save(path)` | Parse/serialize just the JSON side (no NIfTI I/O). |
| `Catalog::load()` | Fetch and parse the public [catalog.json](https://github.com/mrx-org/bifti-phantoms/blob/main/catalog.json) — the discovery list, mapping a label to an immutable registry name. |
| `Registry::load()` | Fetch and parse the public [registry.json](https://github.com/mrx-org/bifti-phantoms/blob/main/registry.json) — the immutable archive of every collection. |
| `Registry::load_registry_phantom(collection, name, cache_dir)` | Download one phantom's JSON + NIfTIs from Zenodo into `cache_dir` (`collection` is an immutable registry name); returns the JSON path. |

## Installation

From [crates.io](https://crates.io/crates/bifti):

```sh
cargo add bifti
```

Or the latest unreleased version from git:

```toml
[dependencies]
bifti = { git = "https://github.com/mrx-org/bifti-phantoms" }
```

## Examples

### `download_random_phantom`

Downloads the public phantom registry, picks a random phantom from a random
collection, downloads it (and the NIfTI files it references) into a local
`cache/` folder, then loads it with `Phantom::load`. Prints `tracing` spans
for the slow steps (downloading, NIfTI loading, mapping-function evaluation),
so it requires the `tracing` feature:

```sh
cargo run --example download_random_phantom --features tracing
```

This prints span timings to the console and also writes `trace.json`
(Chrome Trace Event format) — open it at https://ui.perfetto.dev for a
timeline view of the same spans.

## Features

- `tracing`: instruments the slow code paths (downloading, NIfTI loading,
  mapping-function evaluation) with `tracing` spans. Off by default so the
  library doesn't pull in `tracing` unless you want it.

## Current limitations

- Complex NIfTI data (a complex-valued `B1+`/`B1-` map) is loaded and resliced
  as complex throughout. Only `Float128`, `Complex256`, `Rgb24` and `Rgba32`
  are unsupported, and those fail loudly with `Error::UnsupportedDataType`.
  (The [Python package](https://github.com/mrx-org/bifti-phantoms/blob/main/python/bifti/) still silently drops the
  imaginary part instead.)
- **No implicit reslicing.** Without `reslice_to`, each map keeps its own
  affine and shape; the Python package instead brings every map onto the
  density map's grid.
- Only the default [`units`](https://github.com/mrx-org/bifti-phantoms/blob/main/JSON.md#units) are accepted, matching the
  Python implementation.

See [the top-level README](https://github.com/mrx-org/bifti-phantoms/blob/main/README.md#python-vs-rust) for a fuller
comparison against the Python package.
