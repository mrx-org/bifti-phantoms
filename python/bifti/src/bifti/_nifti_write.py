"""Deterministic NIfTI-1 writer, the same bytes as the Rust, Julia and MATLAB writers.

A phantom directory is hashed as a whole, so the output must be reproducible: fixed
header fields, little-endian, no extensions, and a gzip header without mtime or file name.
nibabel is not used for writing because it would put its own defaults (scaling NaNs,
a ``descrip`` string, the gzip mtime) into the file.
"""

from __future__ import annotations

import gzip
import struct
from pathlib import Path

import numpy as np

HEADER_SIZE = 348
VOX_OFFSET = 352  # header plus the 4-byte (empty) extension flag
GZIP_LEVEL = 6
GZIP_OS_UNKNOWN = 255

# numpy dtype -> (NIfTI datatype code, bitpix)
_DATATYPES: dict[np.dtype, tuple[int, int]] = {
    np.dtype("u1"): (2, 8),
    np.dtype("<i2"): (4, 16),
    np.dtype("<i4"): (8, 32),
    np.dtype("<f4"): (16, 32),
    np.dtype("<c8"): (32, 64),
    np.dtype("<f8"): (64, 64),
    np.dtype("i1"): (256, 8),
    np.dtype("<u2"): (512, 16),
    np.dtype("<u4"): (768, 32),
    np.dtype("<i8"): (1024, 64),
    np.dtype("<u8"): (1280, 64),
    np.dtype("<c16"): (1792, 128),
}


def _stack_dtype(volumes: list[np.ndarray]) -> np.dtype:
    """The common little-endian dtype: kept if all agree, else float64 / complex128."""
    kinds = {v.dtype.newbyteorder("<") for v in volumes}
    if len(kinds) == 1:
        (dtype,) = kinds
        if dtype not in _DATATYPES:
            raise ValueError(f"cannot write NIfTI data of type {dtype}")
        return dtype
    if any(v.dtype.kind == "c" for v in volumes):
        return np.dtype("<c16")
    return np.dtype("<f8")


def build_header(shape, n_volumes: int, affine, dtype: np.dtype) -> bytes:
    """The 352 bytes before the voxel data.

    Orientation is stored in the sform only (``sform_code = 2``, ``qform_code = 0``), as
    ``../../NIFTI.md`` requires.
    """
    affine = np.asarray(affine, dtype=np.float64)[:3]
    datatype, bitpix = _DATATYPES[dtype]
    header = bytearray(VOX_OFFSET)
    struct.pack_into("<i", header, 0, HEADER_SIZE)
    struct.pack_into("<8h", header, 40, 4, *shape, n_volumes, 1, 1, 1)
    struct.pack_into("<hh", header, 70, datatype, bitpix)
    voxel_sizes = np.linalg.norm(affine[:, :3], axis=0)
    struct.pack_into("<5f", header, 76, 1.0, *voxel_sizes.astype(np.float32), 1.0)
    struct.pack_into("<f", header, 108, float(VOX_OFFSET))  # vox_offset
    struct.pack_into("<f", header, 112, 1.0)  # scl_slope
    header[123] = 2  # xyzt_units: millimetres
    struct.pack_into("<hh", header, 252, 0, 2)  # qform_code, sform_code
    struct.pack_into("<12f", header, 280, *affine.astype(np.float32).ravel())
    header[344:348] = b"n+1\0"
    return bytes(header)


def write_stack(path: Path | str, volumes: list[np.ndarray], affine) -> list[int]:
    """Write 3-D ``volumes`` as one 4-D NIfTI-1 file and return each one's 4th-dim index.

    The volumes share ``affine`` and shape. Volumes with identical stored bytes share an
    index and are written once. A path ending in ``.gz`` is gzip-compressed.
    """
    path = Path(path)
    if not volumes:
        raise ValueError(f"{path}: no volumes to write")
    shape = volumes[0].shape
    if len(shape) != 3 or any(v.shape != shape for v in volumes):
        raise ValueError(
            f"{path}: volumes must all be 3-D and share a shape, got "
            f"{sorted({v.shape for v in volumes})}"
        )
    dtype = _stack_dtype(volumes)

    stored: dict[bytes, int] = {}
    slots = []
    for volume in volumes:
        # NIfTI stores voxels x-fastest, which is Fortran order.
        data = np.asarray(volume).astype(dtype, copy=False).ravel(order="F").tobytes()
        slots.append(stored.setdefault(data, len(stored)))

    payload = b"".join(stored)  # dicts keep insertion order = slot order
    blob = build_header(shape, len(stored), affine, dtype) + payload
    if path.suffix == ".gz":
        compressed = bytearray(gzip.compress(blob, compresslevel=GZIP_LEVEL, mtime=0))
        compressed[9] = GZIP_OS_UNKNOWN  # zlib writes the host's OS code
        blob = bytes(compressed)
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(blob)
    return slots


def save_nifti(path: Path | str, volume: np.ndarray, affine) -> None:
    """Write one 3-D volume as a NIfTI-1 file with a singleton 4th dimension."""
    write_stack(path, [volume], affine)
