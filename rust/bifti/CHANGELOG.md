# Changelog

All notable changes to the `bifti` Rust crate are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the crate uses
[semantic versioning](https://semver.org/) (independent of the BIfTI format version).

## [0.2.0] - 2026-10-28

### Changed

- **Breaking:** `Phantom::load(path, max_substeps)` and
  `Phantom::load_from_config(config, base_dir, max_substeps)` take the cap on quadrature
  samples per axis used when an oblique `reslice_to` is resampled, previously fixed at 8.
  Pass `bifti::DEFAULT_MAX_SUBSTEPS` for the old behaviour.
- Resampling onto an oblique `reslice_to` now warns (through `tracing` with the `tracing`
  feature, on stderr otherwise): the result is averaged by approximate midpoint quadrature
  instead of the exact box average used for axis-aligned grids. The warning says when
  `max_substeps` leaves the quadrature under-sampled, so the result may alias
  ([#23](https://github.com/mrx-org/bifti-phantoms/issues/23)).

### Added

- `DEFAULT_MAX_SUBSTEPS` (8).

## [0.1.0] - 2026-10-27

Initial release on crates.io.

### Added

- `Phantom::load` / `load_from_config`: load a phantom JSON and every NIfTI it references
  into one `Tissue` of `Volume`s per tissue, keeping each NIfTI's element type (including
  complex `B1+`/`B1-` maps), with `func` value mappings applied.
- Density-weighted footprint resampling onto `reslice_to`.
- `Phantom::save`, `Volume::save_nifti` / `save_nifti_stack`: deterministic NIfTI-1 output.
- `BiftiPhantom::load` / `.save`: parse and write just the phantom JSON, keeping unknown
  fields so files round-trip.
- Registry access: `Catalog::load`, `Registry::load`, `Registry::load_registry_phantom`.
- Optional `tracing` feature with spans for downloading, NIfTI loading and `func`
  evaluation.
