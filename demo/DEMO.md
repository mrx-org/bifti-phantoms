# Demo

A small, self-contained example that **loads a NIfTI phantom and plots every
tissue's data**. It doubles as a reference for porting the format to your own
code: [`nifti_phantom.py`](nifti_phantom.py) parses the JSON and
[`nifti_loader.py`](nifti_loader.py) turns a phantom into plain NumPy arrays
(see [`../SPEC.md`](../SPEC.md)).

## Files

| File | Purpose |
|------|---------|
| [`nifti_phantom.py`](nifti_phantom.py) | Parse/serialize the phantom JSON (no heavy deps). |
| [`nifti_loader.py`](nifti_loader.py)   | Load a phantom into NumPy arrays (resolves refs, reslices, applies `func`). |
| [`nifti_registry.py`](nifti_registry.py) | Fetch `registry.json` and download phantoms (JSON + NIfTIs) from Zenodo. |
| [`demo.py`](demo.py)                    | Load a phantom and plot one figure per tissue. |
| [`data/`](data/)                        | Phantom JSONs + the NIfTIs they reference. |
| [`data/generate.py`](data/generate.py) | Regenerates the example data in `data/` (reproducible). |

## Requirements

- Python **3.10+**
- [`numpy`](https://numpy.org/), [`nibabel`](https://nipy.org/nibabel/),
  [`scipy`](https://scipy.org/) (used by nibabel for `reslice_to` resampling),
  [`matplotlib`](https://matplotlib.org/) (plotting only), and
  [`requests`](https://requests.readthedocs.io/) (downloading from the registry).

```sh
pip install numpy nibabel scipy matplotlib requests
```

## Running

From this `demo/` directory:

```sh
python demo.py                          # list the registry, pick one, download + plot
python demo.py data/shapes.json         # ...or plot a local phantom JSON directly
python demo.py data/shapes_resliced.json
```

With no argument, `demo.py` fetches the live [`registry.json`](../registry.json),
prints its phantoms as a numbered list, and downloads the one you pick (its JSON
and every NIfTI it references) into `demo/cache/` via
[`nifti_registry.py`](nifti_registry.py) before plotting. Passing a local JSON
path skips the registry and plots that file directly (the example data is
committed in `data/`).

`demo.py` saves one PNG per tissue into `figures/` and, on a GUI backend, also
shows them. Each figure tiles the tissue's maps — `density`, `T1`, `T2`, `T2'`,
`ADC`, `dB0`, and every `B1+`/`B1-` channel — at the volume's middle slice.
Spatially uniform properties show as a flat field; properties left at their
default (e.g. `T1 = inf`) are blanked out.

## The example data

The NIfTIs in `data/` are committed, but they are all produced by
[`data/generate.py`](data/generate.py) — re-run it (`python data/generate.py`)
to regenerate them. It derives everything from a fixed seed and simple
`meshgrid` functions (smooth bumps, low-order polynomials, a little noise), so
re-running reproduces byte-identical files. The phantoms together exercise the
whole format:

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
> the note in [`nifti_loader.py`](nifti_loader.py)).

`demo.py` writes its plots to `figures/`, and `nifti_registry.py` caches
downloads in `cache/`; neither is committed.
