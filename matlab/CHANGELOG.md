# Changelog

All notable changes to the MATLAB / Octave `+bifti` package are documented here. The format
follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the package uses
[semantic versioning](https://semver.org/) (independent of the BIfTI format version). The
package is not published anywhere; the version is recorded in `+bifti/Contents.m`, so
`ver bifti` reports it.

## [0.2.0] - 2026-10-28

### Added

- `bifti.loadPhantom(..., 'MaxSubsteps', n)` (default 8): the cap on quadrature samples per
  axis used when an oblique `reslice_to` is resampled, previously fixed at 8.

### Changed

- Resampling onto an oblique `reslice_to` now warns: the result is averaged by approximate
  midpoint quadrature instead of the exact box average used for axis-aligned grids. The
  warning is `bifti:obliqueResample`, or `bifti:obliqueResampleCapped` when `MaxSubsteps`
  leaves the quadrature under-sampled, so the result may alias
  ([#23](https://github.com/mrx-org/bifti-phantoms/issues/23)).

## [0.1.0] - 2026-10-12

The package as it stood before it was versioned: the MATLAB/Octave implementation
([#26](https://github.com/mrx-org/bifti-phantoms/pull/26)) and saving
([#29](https://github.com/mrx-org/bifti-phantoms/pull/29)). Never tagged.

### Added

- `bifti.readPhantom` / `bifti.writePhantom`: parse and write the phantom JSON, keeping
  unknown fields so files round-trip.
- `bifti.loadPhantom`: load every referenced NIfTI, including complex `B1+`/`B1-` maps and
  `func` value mappings, with density-weighted footprint resampling onto `reslice_to`.
- `bifti.savePhantom`: save a loaded phantom as a deterministic, self-contained folder.
- Patient positions (`bifti.scannerMatrix`, `bifti.scannerAffine`).
- Registry access: `bifti.loadCatalog`, `bifti.loadRegistry`, `bifti.loadRegistryPhantom`
  and `bifti.flattenPhantoms`.
