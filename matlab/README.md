# bifti (MATLAB / Octave)

A MATLAB package for the [BIfTI phantom format](../SPEC.md): parse/serialize the
phantom JSON, load a phantom into plain arrays, and fetch phantoms from the
public [registry](../REGISTRY.md). It needs **no toolboxes** and runs in
**MATLAB R2021a or newer** and **GNU Octave 8 or newer**.

> [!NOTE]
> This package is tested in CI on MATLAB and Octave, but we do not use MATLAB
> much ourselves, so it may be less well maintained than the Python, Rust and
> Julia packages. Feel free to open an
> [issue](https://github.com/mrx-org/bifti-phantoms/issues) or a
> [pull request](https://github.com/mrx-org/bifti-phantoms/pulls) for fixes and
> updates.

```matlab
phantom = bifti.loadPhantom('subj42-3T.json');
gm = phantom.tissues(strcmp({phantom.tissues.name}, 'gm'));
fprintf('%s: mean T1 = %.3f s\n', mat2str(size(gm.density)), mean(gm.T1(:)));
```

## Installation

Clone or [download](https://github.com/mrx-org/bifti-phantoms/archive/refs/heads/main.zip)
the repository and add the `matlab` folder to the path:

```matlab
addpath('/path/to/bifti-phantoms/matlab')
savepath  % optional: keep it for future sessions
```

Everything lives in the `bifti` namespace (`bifti.loadPhantom`, ...), so nothing
clashes with your own functions. `help bifti` lists the API, `help bifti.loadPhantom`
documents a function.

## API at a glance

| Function | What it does |
|----------|--------------|
| `bifti.loadPhantom(path)` | Load a phantom JSON + its NIfTIs: a struct with `config` and `tissues`. |
| `bifti.loadPhantom(config, baseDir)` | Same, for an already parsed (possibly edited) config. |
| `bifti.readPhantom(path)` / `bifti.writePhantom(path, config)` | Parse/serialize just the JSON (no NIfTI I/O). |
| `bifti.niftiFiles(config)` | Every NIfTI file a phantom references. |
| `bifti.scannerMatrix(position)` | The 3×3 phantom RAS+ → scanner rotation of a patient position ([NIFTI.md](../NIFTI.md#patient-position)). |
| `bifti.scannerAffine(phantom, tissueName)` | A tissue's 4×4 voxel → scanner affine. |
| `bifti.loadCatalog()` | Fetch [catalog.json](../catalog.json): label → immutable registry name. |
| `bifti.loadRegistry()` | Fetch [registry.json](../registry.json): every published collection. |
| `bifti.flattenPhantoms(entry)` | All phantom JSON names of a registry entry, out of nested groups. |
| `bifti.loadRegistryPhantom(collection, name)` | Download one phantom's JSON + NIfTIs from Zenodo into a cache (`~/.cache/bifti`); returns the JSON path. |

A loaded tissue has the fields `name`, `density`, `T1`, `T2`, `T2dash`, `ADC`,
`dB0` (3-D arrays indexed `(x, y, z)` like the NIfTI data), `B1_tx`, `B1_rx`
(cell arrays, one array per channel) and `affine` (3×4, voxel → RAS+ in mm for
zero-based indices, as in NIfTI). Units follow [JSON.md](../JSON.md#units).

```matlab
registry = bifti.loadRegistry();
catalog = bifti.loadCatalog();
for k = 1:numel(catalog.keys)
    entry = bifti.internal.jsonGet(registry, catalog.values{k});
    fprintf('%s: %s\n', catalog.keys{k}, strjoin(bifti.flattenPhantoms(entry), ', '));
end
path = bifti.loadRegistryPhantom('endres-bifti_demo-001', 'shapes.json', 'Registry', registry);
phantom = bifti.loadPhantom(path);
```

## Behaviour

- **Grid.** Every map of a tissue is brought onto one grid: `reslice_to` if the
  phantom has one, otherwise the tissue's density grid. Scalars are expanded.
- **Reslicing** is the density-weighted footprint averaging shared with the
  Python, Rust and Julia implementations (see the [top-level README](../README.md#reslicing)).
  The results match the Python package to floating-point precision.
- **Complex data.** Complex `B1+`/`B1-` maps are loaded and resampled as complex;
  every other property must be real.
- **`func` expressions** are parsed with the exact grammar of
  [JSON.md](../JSON.md#transformed-reference) and never `eval`ed, so loading an
  untrusted phantom cannot run code.
- **Unknown fields** are warned about and kept on the phantom and its tissues,
  so `bifti.writePhantom` round-trips them.
- **No toolboxes.** MATLAB's `jsondecode` rewrites keys such as `T2'`, `B1+` and
  `B1-` into field names (the latter two collide), and `niftiread` needs the
  Image Processing Toolbox. The package therefore ships its own small JSON and
  NIfTI-1 readers (`.nii` and `.nii.gz`, either byte order).
- Only the default [`units`](../JSON.md#units) are accepted.

## Tests

The tests load the example phantoms in [`python/bifti/examples/data/`](../python/bifti/examples/data/),
so run them from a checkout of this repository. The same suite runs in MATLAB
and Octave:

```matlab
cd matlab
addpath(pwd); addpath(fullfile(pwd, 'tests'));
runTests
```
