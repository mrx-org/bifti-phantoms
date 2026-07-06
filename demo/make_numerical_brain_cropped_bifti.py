"""Port the legacy MRzero test phantom to the new BIfTI (nifti_phantom_v1) standard.

The legacy phantom `numerical_brain_cropped.mat` is the one that is auto-downloaded
and used by `mr0.util.simulate(...)` when no phantom is passed. It is a cropped,
quantified 2D brain with per-voxel maps for [PD, T1, T2, B0, B1] and uniform
defaults for T2' (0.03 s) and D (1.0). Its physical size is 200 x 200 x 8 mm on a
141 x 161 x 1 grid (see `VoxelGridPhantom.load_mat`).

This script loads that phantom via MRzeroCore and writes it out as a single-segment
BIfTI phantom: one tissue "brain" whose spatially varying properties (PD, T1, T2,
dB0, B1+) are stored as per-voxel NIfTI maps and whose uniform properties (T2', ADC)
are stored as scalars in the JSON sidecar.

Run:  python -m data.make_numerical_brain_cropped_bifti
  or:  python data/make_numerical_brain_cropped_bifti.py
"""

from __future__ import annotations

import json
import os
import tempfile
from pathlib import Path
from urllib.request import urlretrieve

import numpy as np
import nibabel as nib

import MRzeroCore as mr0

DEFAULT_PHANTOM_URL = (
    "https://github.com/MRsources/MRzero-Core/raw/main/"
    "documentation/playground_mr0/numerical_brain_cropped.mat"
)

OUT_NAME = "numerical_brain_cropped_bifti"
OUT_DIR = Path(__file__).parent / OUT_NAME

# Threshold below which a property map is considered uniform and stored as a scalar
STD_THRESHOLD = 1e-5


def load_legacy_phantom() -> mr0.VoxelGridPhantom:
    """Download (if needed) and load the legacy .mat phantom."""
    with tempfile.TemporaryDirectory() as tmp:
        mat_path = os.path.join(tmp, "numerical_brain_cropped.mat")
        print(f"Downloading legacy phantom from {DEFAULT_PHANTOM_URL}")
        urlretrieve(DEFAULT_PHANTOM_URL, mat_path)
        import torch

        # size defaults to [0.2, 0.2, 0.008] m, T2dash=0.03 s, D=1.0. We pass it
        # explicitly as a tensor to work around a list/tensor bug in load_mat.
        phantom = mr0.VoxelGridPhantom.load_mat(
            mat_path, size=torch.tensor([0.2, 0.2, 8e-3])
        )
    return phantom


def to_np(t) -> np.ndarray:
    import torch

    if isinstance(t, torch.Tensor):
        if t.is_complex():
            # Legacy B1 is stored complex with a zero imaginary part
            t = t.real
        return t.detach().cpu().numpy()
    arr = np.asarray(t)
    if np.iscomplexobj(arr):
        arr = arr.real
    return arr


def build_affine(size_m: np.ndarray, shape: tuple[int, int, int]) -> np.ndarray:
    """RAS+ affine (mm) centered at the isocenter, matching data/tissue_dict.py."""
    vs = 1000.0 * size_m / np.asarray(shape)  # voxel size in mm
    return np.array(
        [
            [+vs[0], 0, 0, -size_m[0] / 2 * 1000],
            [0, +vs[1], 0, -size_m[1] / 2 * 1000],
            [0, 0, +vs[2], -size_m[2] / 2 * 1000],
            [0, 0, 0, 1],
        ],
        dtype=np.float64,
    )


def main() -> None:
    phantom = load_legacy_phantom()

    PD = to_np(phantom.PD).astype(np.float32)
    T1 = to_np(phantom.T1).astype(np.float32)
    T2 = to_np(phantom.T2).astype(np.float32)
    T2dash = to_np(phantom.T2dash).astype(np.float32)
    D = to_np(phantom.D).astype(np.float32)
    B0 = to_np(phantom.B0).astype(np.float32)
    B1 = to_np(phantom.B1).astype(np.float32)  # shape (n_coils, X, Y, Z)
    size_m = to_np(phantom.size).astype(np.float64)

    shape = PD.shape
    print(f"Legacy phantom loaded: grid {shape}, size {size_m} m")
    print(
        "Voxel size (mm): "
        f"{1000 * size_m / np.asarray(shape)}"
    )

    OUT_DIR.mkdir(parents=True, exist_ok=True)
    affine = build_affine(size_m, shape)

    # Collect per-property NIfTI channel stacks and the JSON tissue config.
    tissue: dict = {}
    nifti_channels: dict[str, list[np.ndarray]] = {}

    def add_map(prop_key: str, file_suffix: str, data: np.ndarray):
        """Store either a scalar (uniform) or a file_ref (varying) for a property."""
        if float(data.std()) < STD_THRESHOLD:
            tissue[prop_key] = float(data.mean())
            return
        channels = nifti_channels.setdefault(file_suffix, [])
        idx = len(channels)
        channels.append(data.astype(np.float32))
        tissue[prop_key] = f"{OUT_NAME}{file_suffix}.nii.gz[{idx}]"

    # density (PD) is always stored as a map (no suffix)
    nifti_channels[""] = [PD]
    tissue["density"] = f"{OUT_NAME}.nii.gz[0]"

    add_map("T1", "_T1", T1)
    add_map("T2", "_T2", T2)
    add_map("T2'", "_T2'", T2dash)
    add_map("ADC", "_ADC", D)
    add_map("dB0", "_dB0", B0)

    # B1+ transmit channel(s)
    b1_refs: list = []
    for c, channel in enumerate(B1):
        if float(channel.std()) < STD_THRESHOLD:
            b1_refs.append(float(channel.mean()))
        else:
            ch_list = nifti_channels.setdefault("_B1+", [])
            idx = len(ch_list)
            ch_list.append(channel.astype(np.float32))
            b1_refs.append(f"{OUT_NAME}_B1+.nii.gz[{idx}]")
    tissue["B1+"] = b1_refs

    # Write the NIfTI files (stack channels along the 4th dimension)
    for suffix, channels in nifti_channels.items():
        data = np.stack(channels, axis=-1)  # (X, Y, Z, n_channels)
        file_name = OUT_DIR / f"{OUT_NAME}{suffix}.nii.gz"
        print(f"Storing '{file_name}' - {data.shape}")
        nib.save(nib.nifti1.Nifti1Image(data, affine), file_name)

    # Write the JSON sidecar (nifti_phantom_v1)
    config = {
        "file_type": "nifti_phantom_v1",
        "units": {
            "gyro": "MHz/T",
            "B0": "T",
            "T1": "s",
            "T2": "s",
            "T2'": "s",
            "ADC": "10^-3 mm^2/s",
            "dB0": "Hz",
            "B1+": "rel",
            "B1-": "rel",
        },
        "system": {"gyro": 42.5764, "B0": 3},
        "tissues": {"brain": tissue},
    }
    json_path = OUT_DIR / f"{OUT_NAME}.json"
    with open(json_path, "w") as f:
        json.dump(config, f, indent=2)
    print(f"Wrote JSON sidecar '{json_path}'")


if __name__ == "__main__":
    main()
