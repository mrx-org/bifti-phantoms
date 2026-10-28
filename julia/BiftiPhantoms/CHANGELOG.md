# Changelog

All notable changes to BiftiPhantoms.jl are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the package uses
[semantic versioning](https://semver.org/) (independent of the BIfTI format version).

## [0.3.0] - 2026-10-28

### Added

- `save_bifti(...; constant_threshold=...)`: store a map as a constant when its RMS
  deviation from the real part of its mean (its standard deviation, for a real map) is at
  most the threshold. `nothing` (the default) or a negative value disables it; `density`
  always stays a map.

### Changed

- **Breaking:** `save_bifti` prefixes every NIfTI with the name of the JSON file
  (`phantom_density.nii.gz`, `phantom_B1plus_0.nii.gz`, ... for `phantom.json`), so several
  phantoms can be saved into one directory without overwriting each other's maps.

## [0.2.1] - 2026-10-28

### Added

- `load_bifti(...; max_substeps=8)`: the cap on quadrature samples per axis used when an
  oblique `reslice_to` is resampled, previously fixed at 8.

### Changed

- Reslicing onto an oblique `reslice_to` now logs a warning: the result is averaged by
  approximate midpoint quadrature instead of the exact box average used for axis-aligned
  grids. The warning says when `max_substeps` leaves the quadrature under-sampled, so the
  result may alias ([#23](https://github.com/mrx-org/bifti-phantoms/issues/23)).

## [0.2.0] - 2026-10-12

### Added

- `save_bifti` / `save_nifti`: save a loaded `VoxelPhantom` as a deterministic, self-contained
  folder (stacked NIfTI-1 files plus JSON; `func` mappings baked in, no `reslice_to`).

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
