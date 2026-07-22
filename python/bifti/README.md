# bifti

A Python package for the [BIfTI phantom format](../../SPEC.md): parse/serialize
the phantom JSON, load a phantom into plain NumPy arrays, and fetch phantoms
from the public [registry](../../REGISTRY.md).

```python
from bifti import NiftiPhantom, load_phantom

phantom = NiftiPhantom.load("subj42-3T.json")
tissues = load_phantom("subj42-3T.json")  # dict[str, NumpyTissue] of NumPy arrays
```

## Installation

```sh
# using uv (recommended):
uv add --git https://github.com/mrx-org/bifti-phantoms --subdirectory python/bifti bifti
# or using pip:
pip install "git+https://github.com/mrx-org/bifti-phantoms.git#subdirectory=python/bifti"

```

## Examples

[`examples/`](examples/) has a small, self-contained demo that **loads a BIfTI
phantom and plots every tissue's data** — a starting point for seeing the
package in action. 

```sh
git clone https://github.com/mrx-org/bifti-phantoms
uv add --path bifti-phantoms/python/bifti bifti
```

| File | Purpose |
|------|---------|
| [`examples/demo.py`](examples/demo.py) | Load a phantom and plot one figure per tissue. |
| [`examples/nifti_to_koma.py`](examples/nifti_to_koma.py) | Convert a phantom to a KomaMRI `.phantom` HDF5 file. |
| [`examples/mrzero_sim.py`](examples/mrzero_sim.py) | Simulate a Pulseq sequence on a phantom with MR-zero. |
| [`examples/make_numerical_brain_cropped_bifti.py`](examples/make_numerical_brain_cropped_bifti.py) | Port the legacy MRzero test phantom to BIfTI. |
| [`examples/data/`](examples/data/) | Phantom JSONs + the NIfTIs they reference. |
| [`examples/data/generate.py`](examples/data/generate.py) | Regenerates the example data in `examples/data/` (reproducible). |

### Requirements

The core package only needs `bifti`'s own dependencies (numpy, nibabel, scipy,
requests). The examples pull in extra, examples-only dependencies
(`matplotlib`, `h5py`, `MRzeroCore`, `torch`) declared as a
[dependency group](https://docs.astral.sh/uv/concepts/projects/dependencies/#dependency-groups)
rather than real package dependencies:

```sh
uv sync --group examples
```

### Running

From this `python/bifti` directory:

```sh
# list the registry, pick one, download + plot
uv run --group examples examples/demo.py
# ...or plot a local phantom JSON directly
uv run --group examples examples/demo.py examples/data/shapes.json
uv run --group examples examples/demo.py examples/data/shapes_resliced.json
```

With no argument, `demo.py` fetches the live [`registry.json`](../../registry.json),
prints its phantoms as a numbered list, and downloads the one you pick (its JSON
and every NIfTI it references) into `examples/cache/` via `bifti.registry`
before plotting. Passing a local JSON path skips the registry and plots that
file directly (the example data is committed in `examples/data/`).

`demo.py` saves one PNG per tissue into `examples/figures/` and, on a GUI
backend, also shows them. Each figure tiles the tissue's maps — `density`,
`T1`, `T2`, `T2'`, `ADC`, `dB0`, and every `B1+`/`B1-` channel — at the volume's
middle slice. Spatially uniform properties show as a flat field; properties
left at their default (e.g. `T1 = inf`) are blanked out.

## The example data

The NIfTIs in `examples/data/` are committed, but they are all produced by
[`examples/data/generate.py`](examples/data/generate.py) — re-run it
(`uv run --group examples examples/data/generate.py`) to regenerate them. It
derives everything from a fixed seed and simple `meshgrid` functions (smooth
bumps, low-order polynomials, a little noise), so re-running reproduces
byte-identical files. The phantoms together exercise the whole format:

- **`subj42-3T.json`** (hand-written) — a brain-like single slice. Uses
  `reslice_to`, so all maps are resampled from their native 64×64 grid onto a
  100×100 grid on load. Exercises: scalar properties, a shared `dB0` map, an
  **8-channel `B1+`**, and a transformed reference (`fat.dB0 = "x - 420"`).
- **`shapes.json`** (generated) — a small 40×32×4 phantom with **no
  `reslice_to`**, so it loads on its native grid. Exercises: multiple tissues
  sharing one grid, a polynomial `dB0` map, a 2-channel `B1+`, a `func` mapping
  (`"x * 0.5 + 10"`), property defaults, and a true 3D volume.
- **`shapes_resliced.json`** (generated) — the same NIfTIs as `shapes.json` but
  with a `reslice_to` onto a different grid (40×32×4 → **60×48×4**), so loading
  genuinely resamples the 3D volumes.

> **Note:** loading a tissue with a `func` mapping prints a warning — the loader
> evaluates `func` with `eval` for brevity, so only load phantoms you trust (see
> the note in [`bifti/loader.py`](src/bifti/loader.py)).
