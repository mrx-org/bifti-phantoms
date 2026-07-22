# Reference loader for NIfTI phantoms: turns a phantom (parsed by nifti_phantom)
# into plain NumPy arrays. A readable example for porting to your own library -
# not optimised or feature-complete. Deps: numpy, nibabel (and scipy, used by
# nibabel for reslicing). See ../SPEC.md for the format and DEMO.md for usage.

from __future__ import annotations

from dataclasses import dataclass
from functools import lru_cache
from pathlib import Path

import numpy as np
import nibabel
from nibabel.processing import resample_from_to


from .phantom import (
    BiftiPhantom,
    BiftiTissue,
    NiftiRef,
    NiftiMapping,
    ResliceTo,
)


@dataclass
class NumpyTissue:
    """A single tissue with every property resolved to a NumPy array.

    All scalar properties are expanded to full arrays so that downstream code
    can treat uniform and spatially-varying tissues the same way. Every array
    shares the same 3D ``shape`` and the same voxel-to-world ``affine``.

    Units follow the spec (see ``../JSON.md``): T1/T2/T2' in seconds, ADC in
    1e-3 mm^2/s, dB0 in Hz, B1+/B1- relative, density a volume fraction.
    """

    density: np.ndarray  # (X, Y, Z)            volume fraction
    T1: np.ndarray  #      (X, Y, Z)            seconds
    T2: np.ndarray  #      (X, Y, Z)            seconds
    T2dash: np.ndarray  #  (X, Y, Z)            seconds
    ADC: np.ndarray  #     (X, Y, Z)            1e-3 mm^2/s
    dB0: np.ndarray  #     (X, Y, Z)            Hz
    B1_tx: np.ndarray  #   (channels, X, Y, Z)  relative transmit field
    B1_rx: np.ndarray  #   (channels, X, Y, Z)  relative receive field
    resliced: ResliceTo  # Affine + resolution this phantom is resliced to

    @property
    def shape(self) -> list[int]:
        return self.resliced.resolution

    @property
    def affine(self) -> list[list[float]]:
        return self.resliced.affine


# ===========================================================================
# Public entry points
# ===========================================================================


def load_phantom(path: Path | str) -> dict[str, NumpyTissue]:
    """Load a complete phantom from its ``phantom.json``.

    NIfTI files are resolved relative to the JSON file's directory, matching the
    folder convention in ``../SPEC.md``.
    """
    path = Path(path)
    config = BiftiPhantom.load(path)
    return load_config(config, base_dir=path.parent)


def load_config(config: BiftiPhantom, base_dir: Path | str) -> dict[str, NumpyTissue]:
    """Load all tissues of an already-parsed config from ``base_dir``.

    Useful if you want to tweak the config in memory before loading the data.
    """
    base_dir = Path(base_dir)
    return {
        name: load_tissue(tissue, base_dir, config.reslice_to)
        for name, tissue in config.tissues.items()
    }


# ===========================================================================
# Phantom loading internals
# ===========================================================================


def load_tissue(
    tissue: BiftiTissue,
    base_dir: Path | str,
    reslice_to: ResliceTo | None = None,
) -> NumpyTissue:
    """Load one tissue, resolving every property to a NumPy array.

    The output grid is ``reslice_to`` if given, otherwise the density map's own
    grid - so every other map is brought onto the density resolution and affine
    (see ``../JSON.md`` -> ``reslice_to``). The spec requires a phantom's NIfTIs
    to already share that grid, so for conforming data the implicit resampling
    is a no-op.
    """
    base_dir = Path(base_dir)

    if reslice_to is None:
        density, affine = load_file_ref_noreslice(base_dir, tissue.density)
        reslice_to = ResliceTo(affine=affine, resolution=list(density.shape))
    else:
        density = load_file_ref(base_dir, tissue.density, reslice_to)

    def prop(cfg) -> np.ndarray:
        return load_property(cfg, base_dir, reslice_to)

    return NumpyTissue(
        density=density,
        T1=prop(tissue.T1),
        T2=prop(tissue.T2),
        T2dash=prop(tissue.T2dash),
        ADC=prop(tissue.ADC),
        dB0=prop(tissue.dB0),
        B1_tx=np.stack([prop(ch) for ch in tissue.B1_tx], axis=0),
        B1_rx=np.stack([prop(ch) for ch in tissue.B1_rx], axis=0),
        resliced=reslice_to,
    )


def load_property(
    config: float | NiftiRef | NiftiMapping, base_dir: Path, reslice_to: ResliceTo
) -> np.ndarray:
    """Resolve one "scalar-or-map" property to a 3D array (../JSON.md).

    1. a number      -> a uniform array of ``shape`` filled with that value;
    2. a NIfTI ref   -> the referenced sub-volume (resliced onto ``target``);
    3. a transformed -> case 2 with ``func`` applied per voxel.

    Maps come back already on the output grid, so a ``func`` and its ``x_*``
    statistics act on the resliced values.
    """
    if isinstance(config, (int, float)):
        return np.full(reslice_to.resolution, float(config), dtype=np.float64)
    if isinstance(config, NiftiRef):
        return load_file_ref(base_dir, config, reslice_to)
    if isinstance(config, NiftiMapping):
        return eval_expr(config.func, load_file_ref(base_dir, config.file, reslice_to))
    raise TypeError(
        f"property must be a number, NiftiRef or NiftiMapping, got {type(config)}"
    )


# ===========================================================================
# NIfTI file access
# ===========================================================================


def load_file_ref_noreslice(
    base_dir: Path, ref: NiftiRef
) -> tuple[np.ndarray, list[list[float]]]:
    """Load the sub-volume named by ``ref`` on its native grid.

    ``ref`` is a ``"<file>[<index>]"`` reference; ``index`` selects along the
    NIfTI's 4th (tissue) dimension. Returns the 3D sub-volume and the file's own
    3x4 affine (the upper rows of its 4x4 voxel-to-world transform).
    """
    data, affine = _load_nifti(base_dir, ref.file_name, None, None)
    return data[:, :, :, ref.tissue_index], affine


def load_file_ref(base_dir: Path, ref: NiftiRef, reslice_to: ResliceTo) -> np.ndarray:
    """Load the sub-volume named by ``ref`` resliced onto ``reslice_to``.

    The whole file is loaded - and resliced - once (and cached), then the
    requested sub-volume is returned as a 3D array.
    """
    # The cache key must be hashable, so pass the grid as tuples (the model
    # otherwise keeps affines as plain lists; we convert here, ad hoc).
    data, _ = _load_nifti(
        base_dir,
        ref.file_name,
        tuple(reslice_to.resolution),
        tuple(map(tuple, reslice_to.affine)),
    )
    return data[:, :, :, ref.tissue_index]


# Avoid re-loading (and re-reslicing) NIfTIs for every tissue by caching.
@lru_cache(maxsize=20)
def _load_nifti(
    base_dir: Path,
    file_name: Path,
    resolution: tuple[int, ...] | None,
    affine: tuple[tuple[float, ...], ...] | None,
) -> tuple[np.ndarray, list[list[float]]]:
    # A reference may be relative to the phantom directory or an absolute path.
    path = file_name if file_name.is_absolute() else (base_dir / file_name).resolve()
    img = nibabel.load(path)
    assert isinstance(img, nibabel.Nifti1Image)
    assert len(img.shape) == 4

    data = np.asarray(img.dataobj, dtype=np.float64)
    sform = img.get_sform()  # full 4x4 voxel-to-world (RAS+, mm); see ../NIFTI.md
    native_affine = sform[:3].tolist()  # the file's own 3x4 affine

    # No reslicing requested -> return native data and the file's own affine.
    if resolution is None or affine is None:
        return data, native_affine

    # Already on the target grid (the usual case, since the spec requires every
    # NIfTI to share it) -> skip the (lossy, costly) resampling.
    target = [list(row) for row in affine]
    if tuple(data.shape[:3]) == tuple(resolution) and np.allclose(sform[:3], target):
        return data, native_affine

    # Reslice the whole 4D array at once, keeping the 4th (tissue) dimension.
    # `to_vox_map` needs the full 4x4 affine, so re-add the [0, 0, 0, 1] row.
    full = np.array(target + [[0.0, 0.0, 0.0, 1.0]])
    resliced = resample_from_to(
        from_img=nibabel.Nifti1Image(data, sform),
        to_vox_map=(list(resolution) + [data.shape[3]], full),
        order=1,
    )
    return np.asarray(resliced.dataobj, dtype=np.float64), target


# ===========================================================================
# `func` transforms (../JSON.md -> "Transformed reference")
# ===========================================================================


def eval_expr(func: str, data: np.ndarray) -> np.ndarray:
    """Apply a ``func`` transform to a voxel array.

    ``x`` is the per-voxel value; ``x_min``/``x_max``/``x_mean``/``x_std`` are
    scalar statistics of the whole volume.

    NOTE: ``func`` comes straight from the phantom file and is run with ``eval``
    here for brevity, so only load phantoms you trust. The spec restricts it to
    numbers, ``+ - * /``, parentheses and the ``x*`` variables - a hardened
    implementation should parse exactly that grammar instead.
    """
    from warnings import warn

    warn(f"Executing mapping function: '{func}' (possible RCE!)")
    return eval(
        func,
        {"__builtins__": None},
        {
            "x": data,
            "x_min": data.min(),
            "x_max": data.max(),
            "x_mean": data.mean(),
            "x_std": data.std(),
        },
    )


# ===========================================================================
# Example usage
# ===========================================================================

if __name__ == "__main__":
    import sys

    if len(sys.argv) != 2:
        print("usage: python nifti_loader.py <path/to/phantom.json>")
        raise SystemExit(2)

    tissues = load_phantom(sys.argv[1])
    for name, tissue in tissues.items():
        print(f"{name}: shape={tissue.shape}, B1+ channels={tissue.B1_tx.shape[0]}")
        print(
            f"    T1 mean={np.nanmean(tissue.T1):.4g} s, dB0 range="
            f"[{tissue.dB0.min():.4g}, {tissue.dB0.max():.4g}] Hz"
        )
