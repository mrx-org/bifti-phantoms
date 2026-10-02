# Bifti.jl

A Julia package for the [BIfTI phantom format](../../SPEC.md): parse/serialize the
phantom JSON, load a phantom into plain arrays, and fetch phantoms from the public
[registry](../../REGISTRY.md).

```julia
using Bifti

phantom = load_bifti("subj42-3T.json")
tissue = phantom.tissues["gm"]  # a VoxelTissue: density, T1, T2, ... as Array{Float64,3}
println(size(tissue), " ", sum(tissue.T1) / length(tissue.T1))
```

`phantom.config` is the parsed `BiftiPhantom` (the raw JSON structure: system,
patient, tissue definitions) - see [`phantom.jl`](src/phantom.jl) for the data
model and [`loader.jl`](src/loader.jl) for how each property resolves to an array.

## API at a glance

| Name | What it is |
|------|------------|
| `load_bifti(path)` | Load a phantom JSON + its NIfTIs into a `VoxelPhantom(config, tissues)`. |
| `load_bifti(config, base_dir)` | Same, for an already parsed (possibly edited) `BiftiPhantom`. |
| `VoxelTissue` | One tissue as arrays on one grid: `density`, `T1`, `T2`, `T2dash`, `ADC`, `dB0`, `B1_tx`/`B1_rx` (one array per channel), plus `affine` and `size(tissue)`. |
| `read_bifti(path)` / `write_bifti(path, config)` | Parse/serialize just the JSON side (no NIfTI I/O) as a `BiftiPhantom`. |
| `BiftiPhantom`, `BiftiTissue`, `NiftiRef`, `NiftiMapping`, `PhantomSystem`, `Patient`, `ResliceTo` | The JSON data model; construct them with keywords to build phantoms in code. |
| `nifti_files(config)` | Every NIfTI file a phantom references. |
| `scanner_matrix(position_or_config)` | The 3×3 phantom RAS+ → scanner rotation of a patient position ([NIFTI.md](../../NIFTI.md#patient-position)). |
| `scanner_affine(phantom, tissue)` | A tissue's 4×4 voxel → scanner affine. |
| `load_catalog()` | Fetch [catalog.json](../../catalog.json): label → immutable registry name. |
| `load_registry()` | Fetch [registry.json](../../registry.json): every published collection. |
| `flatten_phantoms(entry)` | All phantom JSON names of a registry entry, out of nested groups. |
| `load_registry_phantom(collection, name)` | Download one phantom's JSON + NIfTIs from Zenodo into a cache; returns the JSON path. |

All arrays are indexed `[x, y, z]` like the NIfTI data, and `affine` maps
zero-based voxel indices to RAS+ millimetres, as in NIfTI. Units follow
[JSON.md](../../JSON.md#units): T1/T2/T2' in s, ADC in 10⁻³ mm²/s, dB0 in Hz,
B1± relative.

### Registry

```julia
using Bifti

registry = load_registry()
for (label, collection) in load_catalog()
    println(label, " => ", collection, ": ", flatten_phantoms(registry[collection]))
end

path = load_registry_phantom("endres-bifti_demo-001", "shapes.json"; registry)
phantom = load_bifti(path)
```

Downloads are cached in a [scratch space](https://github.com/JuliaPackaging/Scratch.jl)
of the package (pass `cache_dir` to choose another directory); since a DOI always
resolves to the same bytes, cached files are never downloaded again.

## Installation

```julia
using Pkg
Pkg.add(url="https://github.com/mrx-org/bifti-phantoms", subdir="julia/Bifti")
```

Requires Julia 1.10 or newer.

## Behaviour

- **Grid.** Every map of a tissue is brought onto one grid: `reslice_to` if the
  phantom has one, otherwise the tissue's density grid (which every map of a
  conforming phantom already shares). Scalars are expanded onto that grid.
- **Reslicing** is the density-weighted footprint averaging shared with the
  Python and Rust implementations (see the [top-level README](../../README.md#reslicing)):
  exact box averages for axis-aligned grids, quadrature for oblique ones, linear
  interpolation along axes that are not downsampled. The test suite checks the
  same invariants as theirs, and the results match the Python package to
  floating-point precision.
- **Complex data.** Complex `B1+`/`B1-` maps are loaded and resampled as complex
  (`ComplexF64`); real maps load as `Float64`. Every other property must be real.
- **`func` expressions** are parsed with the exact grammar of
  [JSON.md](../../JSON.md#transformed-reference) and never `eval`ed, so loading an
  untrusted phantom cannot run code. They apply to the resampled values.
- **Unknown fields** are warned about at every level and kept on the phantom and
  its tissues, so `write_bifti` round-trips them.
- Only the default [`units`](../../JSON.md#units) are accepted.

## Tests

The tests load the example phantoms in [`python/bifti/examples/data/`](../../python/bifti/examples/data/),
so run them from a checkout of this repository:

```sh
cd julia/Bifti
julia --project -e 'using Pkg; Pkg.test()'
```

Set `BIFTI_TEST_NETWORK=true` to also download a phantom from Zenodo.
