"""Generate the example NIfTI data in this directory (reproducible).

The ``*.nii.gz`` files (and ``shapes*.json``) next to this script are produced
by running it::

    python generate.py

They are committed for convenience, but re-running reproduces them byte-for-byte
(everything is derived from a fixed seed and simple ``meshgrid`` based functions
- smooth bumps, low-order polynomials, a little noise).

Outputs:

* ``subj42*.nii.gz`` - per-voxel data for the hand-written ``subj42-3T.json``
  (a single axial slice, resliced 64x64 -> 100x100 on load).
* ``shapes*.nii.gz`` + ``shapes.json`` - a small 3D phantom on its native grid.
* ``shapes_resliced.json`` - the same NIfTIs, but with a ``reslice_to`` onto a
  different resolution, so loading actually resamples the 3D volumes.
"""

import sys
from pathlib import Path

import numpy as np
import nibabel

# This script lives in demo/data/; the spec data model lives one level up.
sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from nifti_phantom import (  # noqa: E402
    NiftiPhantom,
    PhantomUnits,
    PhantomSystem,
    NiftiTissue,
    ResliceTo,
)

DATA = Path(__file__).resolve().parent
SEED = 42


def save_nifti(name: str, data: np.ndarray, affine: np.ndarray) -> None:
    """Write a 4D ``(X, Y, Z, tissue)`` float32 NIfTI with a subject-aligned
    sform (``sform_code == 2``, qform unused), as required by ../../NIFTI.md."""
    data = np.ascontiguousarray(data, dtype=np.float32)
    assert data.ndim == 4, "phantom NIfTIs are 4D (4th axis = tissue index)"
    img = nibabel.Nifti1Image(data, affine)
    img.set_sform(affine, code=2)  # 2 == ALIGNED (subject space)
    img.set_qform(None, code=0)
    nibabel.save(img, DATA / name)
    print(f"  wrote {name:24s} shape={data.shape}")


# ---------------------------------------------------------------------------
# subj42: a brain-like single slice, matching subj42-3T.json
# ---------------------------------------------------------------------------


def make_subj42(rng: np.random.Generator) -> None:
    n = 64
    voxel = 220.0 / n  # ~3.4 mm, FOV -110..+110 mm (resliced to 100x100 later)
    affine = np.array(
        [[voxel, 0, 0, -110], [0, voxel, 0, -110], [0, 0, 8.0, 0], [0, 0, 0, 1]]
    )

    rows, cols = np.mgrid[0:n, 0:n].astype(float)
    cy = cx = (n - 1) / 2
    u, v = (cols - cx) / (n / 2), (rows - cy) / (n / 2)
    radius = np.hypot(u, v)
    head = radius < 0.92

    def ring(center, width):
        return np.exp(-(((radius - center) / width) ** 2))

    # Five partial-volume tissue maps (indices match the JSON: gm=0, wm=1, fat=4).
    density = np.stack(
        [
            ring(0.62, 0.16) * head,  # 0 gm  - cortical ribbon
            np.exp(-((radius / 0.45) ** 2)) * head,  # 1 wm  - central
            ring(0.30, 0.07) * head,  # 2 csf - ventricle-ish
            ring(0.86, 0.05),  # 3 skin
            ring(0.80, 0.05),  # 4 fat - subcutaneous rim
        ],
        axis=-1,
    )
    density = np.clip(density + 0.02 * rng.standard_normal(density.shape), 0, 1)
    save_nifti("subj42.nii.gz", density[:, :, None, :], affine)

    # Off-resonance field [Hz]: a smooth low-order polynomial plus a little noise.
    db0 = 30 * u + 20 * v**2 - 15 * u * v + 0.5 * rng.standard_normal((n, n))
    save_nifti("subj42_dB0.nii.gz", (db0 * head)[:, :, None, None], affine)

    # Eight transmit channels: a smooth sensitivity bump per coil around a ring.
    b1 = np.empty((n, n, 8))
    for c in range(8):
        angle = 2 * np.pi * c / 8
        ox, oy = cx + 0.95 * (n / 2) * np.cos(angle), cy + 0.95 * (n / 2) * np.sin(angle)
        b1[:, :, c] = 0.4 + np.exp(-((np.hypot(cols - ox, rows - oy) / n / 0.55) ** 2))
    save_nifti("subj42_B1+.nii.gz", b1[:, :, None, :], affine)


# ---------------------------------------------------------------------------
# shapes: a small fully-generated 3D phantom (json + NIfTIs)
# ---------------------------------------------------------------------------


def make_shapes(rng: np.random.Generator) -> None:
    nx, ny, nz = 40, 32, 4
    affine = np.array([[3, 0, 0, -60], [0, 3, 0, -48], [0, 0, 5.0, -10], [0, 0, 0, 1]])

    xs, ys, zs = np.mgrid[0:nx, 0:ny, 0:nz].astype(float)
    u = (xs - (nx - 1) / 2) / (nx / 2)
    v = (ys - (ny - 1) / 2) / (ny / 2)
    w = (zs - (nz - 1) / 2) / (nz / 2)
    radius = np.sqrt(u**2 + v**2 + w**2)

    # Three tissues sharing one grid: a solid sphere, a shell, a noisy background.
    density = np.stack(
        [
            np.clip(1 - radius / 0.7, 0, 1),
            np.exp(-(((radius - 0.7) / 0.12) ** 2)),
            np.clip(0.15 + 0.05 * rng.standard_normal((nx, ny, nz)), 0, 1),
        ],
        axis=-1,
    )
    save_nifti("shapes_density.nii.gz", density, affine)

    # Polynomial off-resonance field [Hz] and a two-channel transmit field.
    db0 = 40 * u - 25 * v**2 + 15 * u * v + 5 * w
    save_nifti("shapes_dB0.nii.gz", db0[..., None], affine)
    b1 = np.stack([1.1 - 0.4 * u, 0.9 + 0.4 * v], axis=-1)
    save_nifti("shapes_B1.nii.gz", b1, affine)

    # Build the phantom JSON through the spec data model (also exercises writing).
    tissues = {
        "disk": NiftiTissue.from_dict(
            {
                "density": "shapes_density.nii.gz[0]",
                "T1": 1.0,
                "T2": 0.08,
                "T2'": 0.05,
                "ADC": 0.9,
                "dB0": "shapes_dB0.nii.gz[0]",
                "B1+": ["shapes_B1.nii.gz[0]", "shapes_B1.nii.gz[1]"],
            }
        ),
        "ring": NiftiTissue.from_dict(
            {
                "density": "shapes_density.nii.gz[1]",
                "T1": 0.6,
                # A transformed reference: halve the field and shift it by 10 Hz.
                "dB0": {"file": "shapes_dB0.nii.gz[0]", "func": "x * 0.5 + 10"},
            }
        ),
        "background": NiftiTissue.from_dict(
            {"density": "shapes_density.nii.gz[2]", "T1": 4.0, "T2": 2.0}
        ),
    }
    units, system = PhantomUnits.default(), PhantomSystem(42.5764, 3.0)
    NiftiPhantom(units, system, tissues).save(DATA / "shapes.json")
    print("  wrote shapes.json")

    # Same data, but resliced onto a finer 60x48x4 grid (same FOV) - so loading
    # this phantom actually resamples the 3D volumes (40x32 -> 60x48 in-plane).
    reslice_to = ResliceTo(
        affine=[[2, 0, 0, -60], [0, 2, 0, -48], [0, 0, 5, -10]],
        resolution=[60, 48, 4],
    )
    NiftiPhantom(units, system, tissues, reslice_to).save(DATA / "shapes_resliced.json")
    print("  wrote shapes_resliced.json")


def main() -> None:
    rng = np.random.default_rng(SEED)
    print("Generating subj42 (resliced single-slice example) ...")
    make_subj42(rng)
    print("Generating shapes (native + resliced 3D example) ...")
    make_shapes(rng)
    print(f"Done. Files in {DATA}")


if __name__ == "__main__":
    main()
