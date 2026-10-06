# Changelog

All notable changes to BiftiPhantoms.jl are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the package uses
[semantic versioning](https://semver.org/) (independent of the BIfTI format version).

## [0.1.0] - 2026-10-06

Initial release.

### Added

- `read_bifti` / `write_bifti`: parse and serialize the phantom JSON as a `BiftiPhantom`,
  keeping unknown fields so files round-trip.
- `load_bifti`: load every referenced NIfTI into a `VoxelPhantom` of per-tissue arrays,
  including complex multi-channel `B1+`/`B1-` maps and `func` value mappings.
- Density-weighted footprint resampling onto `reslice_to`, matching the Python and
  Rust implementations.
- Patient positions (`scanner_matrix`, `scanner_affine`) for phantom → scanner coordinates.
- Registry access: `load_catalog`, `load_registry`, `load_registry_phantom` (cached
  Zenodo downloads) and `flatten_phantoms`.
