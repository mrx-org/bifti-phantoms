"""Load a NIfTI phantom and plot every tissue's data (one figure per tissue).

Usage::

    python demo.py                # list the registry, pick one, download + plot
    python demo.py data/shapes.json   # ...or plot a local phantom JSON directly

Each figure shows one tissue: its density map, the relaxation/diffusion/off-
resonance maps, and every transmit (B1+) and receive (B1-) channel. 3D volumes
are shown at their middle slice. Figures are saved to ``figures/`` and, if a GUI
backend is available, shown on screen.
"""

from __future__ import annotations

import sys
from pathlib import Path

import numpy as np
import matplotlib.pyplot as plt

from nifti_loader import load_phantom, NumpyTissue
from nifti_registry import available_phantoms, download_phantom

HERE = Path(__file__).parent
FIGURES = HERE / "figures"


def panels(tissue: NumpyTissue) -> list[tuple[str, np.ndarray]]:
    """(label, 3D array) for every property of a tissue, in a fixed order."""
    items = [
        ("density [frac]", tissue.density),
        ("T1 [s]", tissue.T1),
        ("T2 [s]", tissue.T2),
        ("T2' [s]", tissue.T2dash),
        ("ADC [1e-3 mm^2/s]", tissue.ADC),
        ("dB0 [Hz]", tissue.dB0),
    ]
    items += [(f"B1+ ch{i} [rel]", tissue.B1_tx[i]) for i in range(len(tissue.B1_tx))]
    items += [(f"B1- ch{i} [rel]", tissue.B1_rx[i]) for i in range(len(tissue.B1_rx))]
    return items


def plot_tissue(name: str, tissue: NumpyTissue) -> plt.Figure:
    items = panels(tissue)
    ncols = 4
    nrows = -(-len(items) // ncols)  # ceil division
    fig, axes = plt.subplots(
        nrows, ncols, figsize=(3.3 * ncols, 3.3 * nrows), squeeze=False
    )

    z = tissue.density.shape[2] // 2  # middle slice of the (possibly 3D) volume
    for ax, (label, volume) in zip(axes.flat, items):
        # Hide the +inf defaults (e.g. T1/T2 of a tissue that didn't set them).
        sl = np.asarray(volume[:, :, z], dtype=float)
        sl = np.where(np.isfinite(sl), sl, np.nan)
        # .T + origin="lower" so axis 0 (R) runs right and axis 1 (A) runs up.
        im = ax.imshow(sl.T, origin="lower", cmap="viridis")
        ax.set_title(label, fontsize=9)
        ax.set_xticks([])
        ax.set_yticks([])
        fig.colorbar(im, ax=ax, fraction=0.046, pad=0.04)

    for ax in axes.flat[len(items):]:  # blank any unused cells
        ax.axis("off")

    fig.suptitle(
        f"tissue '{name}'  -  slice z={z}, grid {tuple(tissue.shape)}", fontsize=13
    )
    fig.tight_layout()
    return fig


def choose_phantom() -> Path:
    """List the registry's phantoms, ask for one by number, and download it.

    Prints each collection as a bullet header with its phantoms numbered
    continuously across the whole registry; the chosen phantom (JSON + NIfTIs) is
    downloaded from Zenodo and its local JSON path returned.
    """
    index: list[tuple[str, str]] = []
    for collection_name, entry in available_phantoms().items():
        print(f"- {collection_name}")
        for name in entry["phantoms"]:
            index.append((collection_name, name))
            print(f"    {len(index)}. {name}")

    choice = int(input("Select a phantom by number: "))
    collection, name = index[choice - 1]
    print(f"downloading {collection}/{name} ...")
    return download_phantom(collection, name)


def main() -> None:
    if len(sys.argv) > 1:
        json_path = Path(sys.argv[1])
    else:
        json_path = choose_phantom()
    tissues = load_phantom(json_path)

    FIGURES.mkdir(exist_ok=True)
    for name, tissue in tissues.items():
        fig = plot_tissue(name, tissue)
        out = FIGURES / f"{json_path.stem}_{name}.png"
        fig.savefig(out, dpi=110)
        print(f"saved {out}")

    plt.show()


if __name__ == "__main__":
    main()
