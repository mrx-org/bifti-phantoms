# Changelog

All notable changes to the `bifti` Python package are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the package uses
[semantic versioning](https://semver.org/) (independent of the BIfTI format version).

## [0.3.0] - 2026-10-28

### Added

- `NumpyPhantom.save(..., constant_threshold=...)`: store a map as a constant when its RMS
  deviation from the real part of its mean (its standard deviation, for a real map) is at
  most the threshold. `None` (the default) or a negative value disables it; `density`
  always stays a map.

### Changed

- **Breaking:** `NumpyPhantom.save` prefixes every NIfTI with the name of the JSON file
  (`phantom_density.nii.gz`, `phantom_B1plus_0.nii.gz`, ... for `phantom.json`), so several
  phantoms can be saved into one directory without overwriting each other's maps.
- Complex NIfTI data is loaded, resliced and saved as `complex128` instead of losing its
  imaginary part. `B1_tx` / `B1_rx` are complex when their NIfTIs are; a complex `density`,
  `T1`, `T2`, `T2'`, `ADC` or `dB0` raises `ValueError`.

## [0.2.1] - 2026-10-28

### Security

- `func` expressions of transformed references are now parsed against the grammar of
  `JSON.md` and evaluated from the resulting tree instead of being passed to `eval`, so
  loading an untrusted phantom can no longer execute code. Anything outside the grammar
  raises `ValueError`.

## [0.2.0] - 2026-10-28

### Added

- `max_substeps` keyword on `NumpyPhantom.load`, `NumpyPhantom.load_from_config` and
  `load_tissue` (default 8): the cap on quadrature samples per axis used when an oblique
  `reslice_to` is resampled, previously only settable through `Resampler.build`.
- `Resampler.weight_sum` / `resample_weighted` accept a `(C, X, Y, Z)` weight, one per
  channel, so a 4-D phantom NIfTI (one density per tissue) goes through a single call
  ([#23](https://github.com/mrx-org/bifti-phantoms/issues/23)).
- `ObliqueResampleWarning` and `Resampler.capped`.

### Changed

- Resampling onto an oblique grid now emits an `ObliqueResampleWarning`: the result is
  averaged by approximate midpoint quadrature instead of the exact box average used for
  axis-aligned grids. The warning says when `max_substeps` leaves the quadrature
  under-sampled, so the result may alias
  ([#23](https://github.com/mrx-org/bifti-phantoms/issues/23)).
- A weight whose shape matches neither the source grid nor `(C, *source grid)` now raises
  `ValueError` instead of failing inside the contraction.

## [0.1.0] - 2026-10-27

Initial release on PyPI.

### Added

- `NumpyPhantom.load` / `load_from_config`: load a phantom JSON and every NIfTI it
  references into one `NumpyTissue` per tissue, with property defaults and `func` value
  mappings applied.
- Density-weighted footprint resampling onto `reslice_to`, using torch when it is installed
  and NumPy otherwise (`BIFTI_RESAMPLE_BACKEND=numpy|torch`).
- `NumpyPhantom.save`: save a loaded phantom as a deterministic, self-contained folder
  (float64 NIfTIs plus JSON; `func` mappings baked in, no `reslice_to`).
- `BiftiPhantom.load` / `.save`: parse and write just the phantom JSON.
- Patient positions (`patient_to_scanner`, `to_scanner_affine`).
- Registry access: `load_catalog`, `load_registry`, `load_registry_phantom` (cached Zenodo
  downloads).
