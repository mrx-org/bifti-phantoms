# Reference loader for NIfTI phantoms: turns a phantom (parsed by nifti_phantom)
# into plain NumPy arrays. A readable example for porting to your own library -
# not optimised or feature-complete. Deps: numpy, nibabel (and torch, used for
# resampling when it is installed). See ../SPEC.md for the format and DEMO.md
# for usage.

from __future__ import annotations

from dataclasses import dataclass
from functools import lru_cache
from pathlib import Path

import numpy as np
import nibabel

from .resample import Resampler, density_weight, warn_grid_mismatch
from .phantom import (
    BiftiPhantom,
    BiftiTissue,
    NiftiRef,
    NiftiMapping,
    ResliceTo,
)


# ===========================================================================
# Public entry points
# ===========================================================================


@dataclass
class NumpyPhantom:
    """A loaded bifti phantom + maps, represented as dict of [`NumpyTissue`]"""

    config: BiftiPhantom
    tissues: dict[str, NumpyTissue]

    @classmethod
    def load(cls, path: Path | str):
        """Load a complete phantom from its ``phantom.json``.

        NIfTI files are resolved relative to the JSON file's directory, matching the
        folder convention in ``../SPEC.md``.
        """
        path = Path(path)
        config = BiftiPhantom.load(Path(path))
        return cls.load_from_config(config, base_dir=path.parent)

    @classmethod
    def load_from_config(cls, config: BiftiPhantom, base_dir: Path | str):
        """Load all tissues of an already-parsed config from ``base_dir``.

        Useful if you want to tweak the config in memory before loading the data.
        """
        return cls(
            config=config,
            tissues={
                name: load_tissue(tissue, Path(base_dir), config.reslice_to)
                for name, tissue in config.tissues.items()
            },
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
# Phantom loading internals
# ===========================================================================


def load_tissue(
    tissue: BiftiTissue,
    base_dir: Path | str,
    reslice_to: ResliceTo | None = None,
) -> NumpyTissue:
    """Load one tissue, resolving every property to a NumPy array.

    The output grid is ``reslice_to`` if given, otherwise the density map's own grid - so
    every other map is brought onto the density resolution and affine (see ``../JSON.md``
    -> ``reslice_to``). The spec requires a phantom's NIfTIs to already share that grid, so
    for conforming data the implicit resampling is a no-op.

    Resampling averages each output voxel over the source region it covers, weighted by
    this tissue's ``density`` - see :mod:`bifti.resample` for why the weighting matters.
    """
    base_dir = Path(base_dir)

    # The density map defines the source grid *and* supplies the resampling weight, so it
    # is loaded natively first and resampled separately from everything else.
    density_src, src_affine = load_file_ref_noreslice(base_dir, tissue.density)
    src_shape = tuple(density_src.shape)

    if reslice_to is None:
        reslice_to = ResliceTo(affine=src_affine, resolution=list(src_shape))

    resampler = Resampler.build(
        src_affine, src_shape, reslice_to.affine, reslice_to.resolution
    )

    # `density` is a volume fraction - extensive - so it is averaged *unweighted*.
    # Weighting it by itself would renormalise away exactly the partial-volume information
    # the resampling is meant to produce.
    density = resampler.resample_plain(density_src)

    # The denominator every intensive property of this tissue shares.
    weight = density_weight(density_src)
    weight_sum = resampler.weight_sum(weight)

    def prop(cfg, name) -> np.ndarray:
        return load_property(
            cfg, base_dir, reslice_to, resampler, weight, weight_sum, src_affine,
            src_shape, name,
        )

    def channels(cfgs, name) -> np.ndarray:
        return np.stack(
            [prop(ch, f"{name}[{i}]") for i, ch in enumerate(cfgs)], axis=0
        )

    return NumpyTissue(
        density=density,
        T1=prop(tissue.T1, "T1"),
        T2=prop(tissue.T2, "T2"),
        T2dash=prop(tissue.T2dash, "T2'"),
        ADC=prop(tissue.ADC, "ADC"),
        dB0=prop(tissue.dB0, "dB0"),
        B1_tx=channels(tissue.B1_tx, "B1+"),
        B1_rx=channels(tissue.B1_rx, "B1-"),
        resliced=reslice_to,
    )


def load_property(
    config: float | NiftiRef | NiftiMapping,
    base_dir: Path,
    reslice_to: ResliceTo,
    resampler: Resampler,
    weight: np.ndarray,
    weight_sum: np.ndarray,
    src_affine: list[list[float]],
    src_shape: tuple[int, ...],
    name: str,
) -> np.ndarray:
    """Resolve one "scalar-or-map" property to a 3D array (../JSON.md).

    1. a number      -> a uniform array of ``shape`` filled with that value;
    2. a NIfTI ref   -> the referenced sub-volume, density-weighted onto the target grid;
    3. a transformed -> case 2 with ``func`` applied per voxel.

    Maps come back already on the output grid, so a ``func`` and its ``x_*`` statistics act
    on the resampled values.
    """
    if isinstance(config, (int, float)):
        # A uniform value needs no resampling - just fill the target grid.
        return np.full(reslice_to.resolution, float(config), dtype=np.float64)
    if isinstance(config, NiftiRef):
        ref = config
    elif isinstance(config, NiftiMapping):
        ref = config.file
    else:
        raise TypeError(
            f"property must be a number, NiftiRef or NiftiMapping, got {type(config)}"
        )

    native, native_affine = load_file_ref_noreslice(base_dir, ref)
    if tuple(native.shape) == src_shape and np.allclose(native_affine, src_affine):
        data = resampler.resample_weighted(native, weight, weight_sum)
    else:
        # ../NIFTI.md requires every NIfTI of a phantom to share a grid. A non-conforming
        # file cannot use the density weight, so fall back to an unweighted average.
        warn_grid_mismatch(name)
        other = Resampler.build(
            native_affine, native.shape, reslice_to.affine, reslice_to.resolution
        )
        data = other.resample_plain(native)

    if isinstance(config, NiftiMapping):
        return eval_expr(config.func, data)
    return data


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
    data, affine = _load_nifti(base_dir, ref.file_name)
    return data[:, :, :, ref.tissue_index], affine


# Avoid re-loading NIfTIs for every tissue by caching the native (unresampled) file.
# Resampling can no longer be cached per file: it depends on the *tissue's* density map,
# which generally lives in a different file. Caching the raw load instead is if anything a
# stronger cache, since it no longer varies with the target grid.
@lru_cache(maxsize=20)
def _load_nifti(
    base_dir: Path, file_name: Path
) -> tuple[np.ndarray, list[list[float]]]:
    # A reference may be relative to the phantom directory or an absolute path.
    path = file_name if file_name.is_absolute() else (base_dir / file_name).resolve()
    img = nibabel.load(path)
    assert isinstance(img, nibabel.Nifti1Image)
    assert len(img.shape) == 4

    data = np.asarray(img.dataobj, dtype=np.float64)
    sform = img.get_sform()  # full 4x4 voxel-to-world (RAS+, mm); see ../NIFTI.md
    return data, sform[:3].tolist()


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

    phantom = NumpyPhantom.load(sys.argv[1])
    for name, tissue in phantom.tissues.items():
        print(f"{name}: shape={tissue.shape}, B1+ channels={tissue.B1_tx.shape[0]}")
        print(
            f"    T1 mean={np.nanmean(tissue.T1):.4g} s, dB0 range="
            f"[{tissue.dB0.min():.4g}, {tissue.dB0.max():.4g}] Hz"
        )
