# bifti

A Rust crate for the [BIfTI phantom format](../../SPEC.md): parse/serialize the
phantom JSON, load a phantom into plain arrays, and fetch phantoms from the
public [registry](../../REGISTRY.md).

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
system, tissue definitions) — see [`phantom.rs`](src/phantom.rs) for the full
data model and [`loader.rs`](src/loader.rs) for how each property resolves to
a `Volume`.

## API at a glance

| Item | What it is |
|------|------------|
| `Phantom::load(path)` | Load a phantom JSON + its NIfTIs into `Phantom { config, tissues }`. |
| `Tissue` | One tissue as `Volume`s: `density`, `t1`, `t2`, `t2dash`, `adc`, `db0`, `b1_tx: Vec<Volume>`, `b1_rx: Vec<Volume>`. |
| `Volume` | `{ affine: [[f64; 4]; 3], shape: [usize; 3], data: VolumeData }`. |
| `VolumeData` | `Float32(Vec<f32>)` / `Float64(Vec<f64>)` of the volume's voxels, row-major (`x*ny*nz + y*nz + z`). |
| `BiftiPhantom::load(path)` / `.save(path)` | Parse/serialize just the JSON side (no NIfTI I/O). |
| `Registry::load()` | Fetch and parse the public [registry.json](../../registry.json). |
| `Registry::load_registry_phantom(collection, name, cache_dir)` | Download one phantom's JSON + NIfTIs from Zenodo into `cache_dir`; returns the JSON path. |

## Installation

```sh
cargo add --git https://github.com/mrx-org/bifti-phantoms bifti
```

Or manually in `Cargo.toml`:

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

- **No complex NIfTI data.** `VolumeData::Complex32`/`Complex64` exist as
  variants but nothing produces them yet — a complex-valued `B1+`/`B1-` map
  (`NiftiType::Complex64`/`Complex128`) fails loudly with
  `Error::UnsupportedDataType`. (The [Python package](../../python/bifti/)
  doesn't handle this correctly either — it silently drops the imaginary part
  instead of erroring, so a hard failure here is arguably the safer gap to
  have.)
- **No reslicing for complex data**, for the same reason — `Volume::reslice`
  only handles the real-valued variants.
- Only the default [`units`](../../JSON.md#units) are accepted, matching the
  Python implementation.

See [the top-level README](../../README.md#python-vs-rust) for a fuller
comparison against the Python package.
