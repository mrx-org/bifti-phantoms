"""Convert a NIfTI phantom (JSON + NIfTIs) to a KomaMRI ``.phantom`` HDF5 file.

The output follows KomaMRIFiles' on-disk schema (see KomaMRIFiles' ``Phantom.jl``):

    root attrs:  Version, Name, Ns, Dims
    /position/   x, y, z              (Float64, length Ns, meters)
    /contrast/   ρ, T1, T2, T2s, Δw   (Float64, length Ns; T in s, Δw in rad/s)

Usage::

    python nifti_to_koma.py data/subj42-3T.json
    python nifti_to_koma.py data/subj42-3T.json --out subj42.phantom

The result is loadable in Julia via ``KomaMRIFiles.read_phantom(path)``.

Approximations / what's dropped
------------------------------
Koma's contrast group has no slot for ADC, B1-, or multi-channel B1+. To stay
within the format:

* **ADC** (diffusion) is dropped entirely.
* **B1-** (receive sensitivity) is dropped entirely; warned if any tissue
  sets it to non-default.
* **B1+** uses only the first channel; warned if more channels exist. To keep
  the first channel from being silently lost, it is *baked into* ρ as
  ``ρ_spin = density * B1+[0]``. This is exact for the received-signal scaling
  but only approximate for the excitation flip-angle, since the Koma simulator
  does not see B1+ separately (sin(α·B1) ≠ B1·sin(α) for large α).

NIfTI affine is in mm (RAS+); positions are converted to meters.
T2* is built from ``1/T2s = 1/T2 + 1/T2'`` (handles inf cases via numpy).
Δw is ``2π · dB0`` (Hz → rad/s).

Dependencies: numpy, nibabel, scipy (transitive), h5py.
"""

from __future__ import annotations

import argparse
import sys
import warnings
from pathlib import Path

import numpy as np
import h5py

from nifti_loader import load_phantom, NumpyTissue


def tissue_spins(tissue: NumpyTissue, density_threshold: float,
                 spins_per_voxel: int, rng: np.random.Generator
                 ) -> dict[str, np.ndarray]:
    """Pick the active voxels of one tissue and return per-spin arrays.

    With ``spins_per_voxel > 1`` each voxel emits that many spins, jittered
    uniformly inside the voxel box (offset in [-0.5, +0.5] in voxel indices),
    and ρ is divided by the spin count so the voxel's total magnetisation is
    preserved. ``spins_per_voxel == 1`` keeps the old behaviour (one spin at
    the voxel centre).

    Output keys: x, y, z (meters), rho, T1, T2, T2s (seconds), dw (rad/s).
    """
    # Mask: keep voxels where this tissue actually contributes.
    mask = tissue.density > density_threshold
    if not mask.any():
        return {k: np.empty(0, dtype=np.float64) for k in
                ("x", "y", "z", "rho", "T1", "T2", "T2s", "dw")}

    S = spins_per_voxel
    # Voxel indices -> world coordinates via the file's 3x4 sform (mm, RAS+).
    # The affine has shape [3][4]: [R | t], so (x,y,z) = R @ (i,j,k) + t.
    affine = np.asarray(tissue.affine, dtype=np.float64)
    R, t = affine[:, :3], affine[:, 3]
    i, j, k = np.where(mask)
    N = i.size
    voxel_idx = np.stack([i, j, k], axis=0).astype(np.float64)  # (3, N)
    if S == 1:
        idx = voxel_idx
    else:
        # Repeat each voxel S times, then jitter uniformly inside its box.
        idx = np.broadcast_to(voxel_idx[:, :, None], (3, N, S)).copy()
        idx += rng.uniform(-0.5, 0.5, size=(3, N, S))
        idx = idx.reshape(3, N * S)
    world_mm = R @ idx + t[:, None]
    x, y, z = world_mm / 1000.0  # mm -> m

    # Per-voxel properties, then spread each value across the voxel's S spins.
    density_v = tissue.density[mask].astype(np.float64)
    b1_v = tissue.B1_tx[0][mask].astype(np.float64)  # bake first B1+ into ρ
    T1_v = tissue.T1[mask].astype(np.float64)
    T2_v = tissue.T2[mask].astype(np.float64)
    T2dash_v = tissue.T2dash[mask].astype(np.float64)
    dB0_v = tissue.dB0[mask].astype(np.float64)
    # T2* = 1 / (1/T2 + 1/T2'); numpy yields 1/inf=0 and 1/0=inf which is exactly
    # the right limit, but emits warnings -- silence them just for this block.
    with np.errstate(divide="ignore", invalid="ignore"):
        T2s_v = 1.0 / (1.0 / T2_v + 1.0 / T2dash_v)

    # /S preserves total magnetisation when spreading across multiple spins.
    rho = np.repeat(density_v * b1_v, S) / S
    T1 = np.repeat(T1_v, S)
    T2 = np.repeat(T2_v, S)
    T2s = np.repeat(T2s_v, S)
    dw = np.repeat(2.0 * np.pi * dB0_v, S)  # Hz -> rad/s

    return {"x": x, "y": y, "z": z,
            "rho": rho, "T1": T1, "T2": T2, "T2s": T2s, "dw": dw}


def warn_on_drops(tissues: dict[str, NumpyTissue]) -> None:
    """Emit warnings for the features we silently drop or fold into ρ."""
    for name, t in tissues.items():
        # Extra B1+ channels beyond the first.
        if t.B1_tx.shape[0] > 1:
            warnings.warn(
                f"tissue '{name}': {t.B1_tx.shape[0]} B1+ channels found; "
                f"only channel 0 used (baked into ρ), the rest are dropped.",
                stacklevel=2,
            )
        # Any B1- channel that isn't the spec default of uniform 1.0.
        for ch, vol in enumerate(t.B1_rx):
            finite = vol[np.isfinite(vol)]
            if finite.size and not np.allclose(finite, 1.0):
                warnings.warn(
                    f"tissue '{name}': B1- channel {ch} is not uniform 1.0; "
                    f"Koma's .phantom has no slot for B1-, so it is dropped.",
                    stacklevel=2,
                )
        # ADC (diffusion) -- only worth a warning if it isn't 0 everywhere.
        adc_finite = t.ADC[np.isfinite(t.ADC)]
        if adc_finite.size and not np.allclose(adc_finite, 0.0):
            warnings.warn(
                f"tissue '{name}': ADC is non-zero; Koma's .phantom has no "
                f"slot for diffusion, so ADC is dropped.",
                stacklevel=2,
            )


def write_koma_phantom(path: Path, name: str, spins: dict[str, np.ndarray]) -> None:
    """Write the flat spin list to a Koma-compatible .phantom HDF5 file."""
    Ns = spins["x"].size
    # Koma's get_dims: which of (x, y, z) carry any non-zero entry.
    dims = [bool(np.any(spins[axis] != 0.0)) for axis in ("x", "y", "z")]
    if not any(dims):
        dims = [True, False, False]  # match Koma's fallback

    with h5py.File(path, "w") as fid:
        # Koma parses this via Julia's VersionNumber (SemVer), and warns on a
        # major-version mismatch against pkgversion(KomaMRIFiles). Major 0
        # matches current Koma; the build metadata tags this file's source.
        fid.attrs["Version"] = "0.1.0+nifti-to-koma"
        fid.attrs["Name"] = name
        fid.attrs["Ns"] = Ns
        fid.attrs["Dims"] = int(sum(dims))

        position = fid.create_group("position")
        for axis in ("x", "y", "z"):
            position.create_dataset(axis, data=spins[axis])

        contrast = fid.create_group("contrast")
        # Use the exact Unicode names Koma writes/reads.
        contrast.create_dataset("ρ", data=spins["rho"])
        contrast.create_dataset("T1", data=spins["T1"])
        contrast.create_dataset("T2", data=spins["T2"])
        contrast.create_dataset("T2s", data=spins["T2s"])
        contrast.create_dataset("Δw", data=spins["dw"])


def convert(json_path: Path, out_path: Path, name: str,
            density_threshold: float, spins_per_voxel: int,
            seed: int | None) -> None:
    if spins_per_voxel < 1:
        raise ValueError("spins_per_voxel must be >= 1")
    rng = np.random.default_rng(seed)
    tissues = load_phantom(json_path)
    warn_on_drops(tissues)

    per_tissue = [tissue_spins(t, density_threshold, spins_per_voxel, rng)
                  for t in tissues.values()]
    spins = {key: np.concatenate([t[key] for t in per_tissue])
             for key in ("x", "y", "z", "rho", "T1", "T2", "T2s", "dw")}

    write_koma_phantom(out_path, name, spins)
    print(f"wrote {out_path}  ({spins['x'].size} spins from "
          f"{len(tissues)} tissues, {spins_per_voxel} spins/voxel)")


def main() -> None:
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    p.add_argument("json", type=Path, help="path to the phantom JSON file")
    p.add_argument("--out", type=Path, default=None,
                   help="output .phantom path (default: <json>.phantom next to the json)")
    p.add_argument("--name", default=None,
                   help="phantom Name attribute (default: JSON stem)")
    p.add_argument("--density-threshold", type=float, default=1e-3,
                   help="drop voxels with density below this (default: 1e-3)")
    p.add_argument("--spins-per-voxel", type=int, default=100,
                   help="spins emitted per active voxel; >1 jitters spins "
                        "uniformly inside the voxel box and divides rho by "
                        "the count to preserve total magnetisation (default: 100)")
    p.add_argument("--seed", type=int, default=None,
                   help="RNG seed for the jitter (default: nondeterministic)")
    args = p.parse_args()

    out_path = args.out if args.out is not None else args.json.with_suffix(".phantom")
    name = args.name if args.name is not None else args.json.stem
    convert(args.json, out_path, name, args.density_threshold,
            args.spins_per_voxel, args.seed)


if __name__ == "__main__":
    main()
