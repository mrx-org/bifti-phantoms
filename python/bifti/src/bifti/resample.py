"""Density-weighted footprint resampling onto a new voxel grid.

Plain interpolation only ever reads the voxels immediately around a sample point, so
resampling onto a *coarser* grid throws most of the source data away and aliases. This
module instead averages each output voxel over the whole source region it covers.

Averaging naively would be wrong for a phantom, though: outside the source FOV - and in the
background between tissues - a map reads 0, and T1, T2, dB0 and friends are *intensive*
quantities. Averaging them against those zeros drags them towards zero at every edge. Only
``density`` is extensive. So every intensive property is averaged **weighted by the tissue's
density**::

    P_out = sum_j w_j*W_j*P_j / sum_j w_j*W_j    with W = density + eps
    D_out = sum_j w_j*D_j

Sampling ``W`` as a volume in its own right is what makes this work: taps that fall outside
the source contribute to *neither* sum, so they are excluded rather than averaged in. Where
a footprint is entirely empty the ``eps`` terms are uniform and the result falls back to the
unweighted mean of the in-bounds taps, so it is never ``NaN``.

For axis-aligned grids - which is every affine ``../../JSON.md``'s ``reslice_to`` is used
with in practice - the footprint average factorises into three 1-D passes, each a matrix
contraction, and is computed *exactly*: a true box average with no quadrature error and no
substep cap. Oblique transforms fall back to midpoint quadrature over the output voxel's
parallelepiped.
"""

from __future__ import annotations

import warnings

import numpy as np

from ._backend import Backend, get_backend

# Substep cap for the oblique fallback. The separable path is exact and needs no cap.
DEFAULT_MAX_SUBSTEPS = 8

# Relative size below which a matrix entry counts as zero when checking axis alignment.
# Far below any real obliquity (a 1 degree rotation already gives ~0.017).
AXIS_ALIGNED_TOL = 1e-9

# Weights below this (of a footprint that sums to 1) are rounding crumbs, not overlap.
#
# Inverting the source affine is inexact, so a voxel that only *touches* the footprint -
# zero-width overlap - can come out with a weight of ~1e-17 instead of 0. Left in, such a
# tap makes the total weight non-zero, and the weighted mean then divides a crumb by a
# crumb and reports that lone voxel's value as if it were the average of the footprint.
WEIGHT_EPS = 1e-12


def _as_4x4(affine) -> np.ndarray:
    """A 3x4 affine (as stored in ``reslice_to``) as a full 4x4 matrix."""
    a = np.asarray(affine, dtype=np.float64)
    if a.shape == (4, 4):
        return a
    return np.vstack([a, [0.0, 0.0, 0.0, 1.0]])


def axis_matrix(n_out: int, n_src: int, scale: float, offset: float) -> np.ndarray:
    """The ``(n_out, n_src)`` resampling matrix for one axis.

    ``scale`` and ``offset`` map an output index to a source coordinate as
    ``c = scale*i + offset``. Rows sum to 1 over the *unclipped* footprint, so an output
    voxel half outside the source keeps half its density and the phantom fades out at the
    FOV edge instead of ending abruptly. Out-of-bounds taps are simply dropped, which is
    what excludes them from both sums above.
    """
    a = np.zeros((n_out, n_src), dtype=np.float64)
    i = np.arange(n_out, dtype=np.float64)
    j = np.arange(n_src, dtype=np.float64)

    if abs(scale) > 1.0:
        # Downsampling: exact box (area) weights - the overlap between the output voxel's
        # footprint [lo, hi] and each source voxel's extent [j-0.5, j+0.5].
        c0 = scale * (i - 0.5) + offset
        c1 = scale * (i + 0.5) + offset
        lo = np.minimum(c0, c1)[:, None]
        hi = np.maximum(c0, c1)[:, None]
        overlap = np.minimum(hi, j + 0.5) - np.maximum(lo, j - 0.5)
        a = np.clip(overlap, 0.0, None) / abs(scale)
    else:
        # Not downsampling: two-tap linear weights, i.e. exactly what trilinear
        # interpolation does, so this path's behaviour is unchanged.
        c = scale * i + offset
        j0 = np.floor(c).astype(np.int64)
        f = c - j0
        for tap, weight in ((j0, 1.0 - f), (j0 + 1, f)):
            inside = (tap >= 0) & (tap < n_src)
            a[np.nonzero(inside)[0], tap[inside]] += weight[inside]

    a[a < WEIGHT_EPS] = 0.0
    return a


def decompose_axis_aligned(m: np.ndarray):
    """``(src_axis, scale, offset)`` per output axis, or ``None`` for an oblique ``m``.

    ``m`` maps an output voxel index to a continuous source voxel index.
    """
    src_axis, scale, offset = [], [], []
    for a in range(3):
        col = m[:3, a]
        norm = float(np.linalg.norm(col))
        if norm == 0.0:
            return None  # degenerate: the output axis maps to a point
        s = int(np.argmax(np.abs(col)))
        # Every other entry of the column must be negligible for the axes to separate.
        if np.any(np.abs(np.delete(col, s)) > AXIS_ALIGNED_TOL * norm):
            return None
        src_axis.append(s)
        scale.append(float(col[s]))
        offset.append(float(m[s, 3]))

    # The axis assignment has to be a permutation, else two output axes share a source axis.
    if sorted(src_axis) != [0, 1, 2]:
        return None
    return src_axis, scale, offset


def _oblique_substeps(m: np.ndarray, max_substeps: int) -> tuple[int, int, int]:
    """How finely to sample each output voxel along each axis, for the oblique path."""
    steps = []
    for a in range(3):
        span = float(np.linalg.norm(m[:3, a]))
        steps.append(1 if span <= 1.0 else min(max_substeps, max(1, int(np.ceil(span)))))
    return tuple(steps)


class Resampler:
    """A precomputed source-grid -> target-grid resampler, shared by a tissue's maps.

    Build it once per tissue and reuse it for `density` and every property: the tap tables
    depend only on the two grids, and the weight denominator only on the density map.
    """

    def __init__(self, kind: str, src_shape, out_shape, backend: Backend, **kw):
        self.kind = kind
        self.src_shape = tuple(src_shape)
        self.out_shape = tuple(out_shape)
        self.backend = backend
        self.__dict__.update(kw)

    @classmethod
    def build(
        cls,
        src_affine,
        src_shape,
        dst_affine,
        dst_shape,
        *,
        max_substeps: int = DEFAULT_MAX_SUBSTEPS,
        backend: Backend | None = None,
    ) -> Resampler:
        backend = backend or get_backend()
        src_affine = _as_4x4(src_affine)
        dst_affine = _as_4x4(dst_affine)
        src_shape = tuple(int(s) for s in src_shape)
        dst_shape = tuple(int(s) for s in dst_shape)

        if src_shape == dst_shape and np.allclose(src_affine, dst_affine):
            return cls("identity", src_shape, dst_shape, backend)

        # Output voxel index -> world -> continuous source voxel index.
        m = np.linalg.inv(src_affine) @ dst_affine

        decomposed = decompose_axis_aligned(m)
        if decomposed is None:
            return cls(
                "oblique",
                src_shape,
                dst_shape,
                backend,
                m=m,
                substeps=_oblique_substeps(m, max_substeps),
            )

        src_axis, scale, offset = decomposed
        # Matrices indexed by *source* axis (the contraction order); each contracts that
        # source axis down to the output axis mapped onto it.
        mats: list[np.ndarray | None] = [None, None, None]
        for a in range(3):
            s = src_axis[a]
            mats[s] = axis_matrix(dst_shape[a], src_shape[s], scale[a], offset[a])
        return cls(
            "separable",
            src_shape,
            dst_shape,
            backend,
            mats=[backend.asarray(x) for x in mats],
            src_axis=src_axis,
        )

    @property
    def is_identity(self) -> bool:
        return self.kind == "identity"

    def _contract(self, vol: np.ndarray) -> np.ndarray:
        """``sum_j w_j * x_j`` per output voxel, for a ``(C, X, Y, Z)`` array."""
        if self.kind == "identity":
            return vol
        if self.kind == "oblique":
            # Rare fallback for non-conforming affines - kept on NumPy so no backend needs
            # a scattered-gather op of its own.
            return self._contract_oblique(vol)
        # Separable: contract one source axis at a time. tensordot puts the new axis at
        # the front, so move it back into place each time.
        xp = self.backend
        vol = xp.asarray(vol)
        for s in range(3):
            # source axis `s` sits at position s+1, since axis 0 is the channel axis
            vol = xp.tensordot(self.mats[s], vol, axes=([1], [s + 1]))
            vol = xp.moveaxis(vol, 0, s + 1)
        if self.src_axis != [0, 1, 2]:
            # The result is still indexed by source-axis position; put it in output order.
            # `src_axis[a]` is the source axis that output axis `a` came from.
            vol = xp.transpose(vol, [0, *(1 + self.src_axis[a] for a in range(3))])
        return xp.to_numpy(vol)

    def _contract_oblique(self, vol: np.ndarray) -> np.ndarray:
        """Midpoint quadrature over each output voxel's parallelepiped."""
        m, substeps = self.m, self.substeps
        nx, ny, nz = self.out_shape

        # Sub-sample offsets inside an output voxel, in *output* index space, so the
        # quadrature follows the oblique parallelepiped the voxel actually maps to.
        def offsets(n):
            return -0.5 + (np.arange(n, dtype=np.float64) + 0.5) / n

        grids = np.meshgrid(
            np.arange(nx, dtype=np.float64),
            np.arange(ny, dtype=np.float64),
            np.arange(nz, dtype=np.float64),
            indexing="ij",
        )
        acc = None
        for dx in offsets(substeps[0]):
            for dy in offsets(substeps[1]):
                for dz in offsets(substeps[2]):
                    p = [grids[0] + dx, grids[1] + dy, grids[2] + dz]
                    c = [
                        m[k, 0] * p[0] + m[k, 1] * p[1] + m[k, 2] * p[2] + m[k, 3]
                        for k in range(3)
                    ]
                    sample = _trilinear(vol, c)
                    acc = sample if acc is None else acc + sample
        return acc / float(np.prod(substeps))

    def weight_sum(self, weight: np.ndarray) -> np.ndarray:
        """``sum_j w_j * weight_j`` per output voxel.

        This is the denominator every property of a tissue shares, so compute it once and
        hand it to :meth:`resample_weighted`.
        """
        return self._contract(np.asarray(weight, dtype=np.float64)[None])[0]

    def resample_plain(self, data: np.ndarray) -> np.ndarray:
        """Unweighted footprint average - the correct rule for ``density``, which is
        extensive. Accepts ``(X, Y, Z)`` or ``(C, X, Y, Z)``."""
        vol, squeeze = _with_channels(data)
        out = self._contract(vol)
        return out[0] if squeeze else out

    def resample_weighted(
        self, data: np.ndarray, weight: np.ndarray, weight_sum: np.ndarray
    ) -> np.ndarray:
        """Density-weighted footprint average - the correct rule for intensive properties.

        Accepts ``(X, Y, Z)`` or ``(C, X, Y, Z)``; all channels of a ``B1+``/``B1-`` map go
        through a single contraction.
        """
        vol, squeeze = _with_channels(data)
        num = self._contract(vol * np.asarray(weight, dtype=np.float64)[None])
        # weight_sum <= 0 means not one in-bounds tap: nothing to say about the value.
        out = np.divide(num, weight_sum[None], out=np.zeros_like(num), where=weight_sum > 0)
        return out[0] if squeeze else out


def _trilinear(vol: np.ndarray, coords) -> np.ndarray:
    """Trilinear sample of a ``(C, X, Y, Z)`` volume; outside the volume reads as zero."""
    shape = vol.shape[1:]
    base = [np.floor(c) for c in coords]
    frac = [c - b for c, b in zip(coords, base)]
    base = [b.astype(np.int64) for b in base]

    acc = np.zeros((vol.shape[0], *coords[0].shape), dtype=np.float64)
    for dx in (0, 1):
        for dy in (0, 1):
            for dz in (0, 1):
                idx = [base[0] + dx, base[1] + dy, base[2] + dz]
                w = np.ones_like(frac[0])
                for d, delta in enumerate((dx, dy, dz)):
                    w = w * (frac[d] if delta else 1.0 - frac[d])
                inside = np.ones_like(w, dtype=bool)
                for d in range(3):
                    inside &= (idx[d] >= 0) & (idx[d] < shape[d])
                clipped = [np.clip(idx[d], 0, shape[d] - 1) for d in range(3)]
                acc += np.where(inside, w, 0.0) * vol[:, clipped[0], clipped[1], clipped[2]]
    return acc


def _with_channels(data: np.ndarray) -> tuple[np.ndarray, bool]:
    """Give a 3-D map a leading singleton channel axis; report whether to strip it again."""
    data = np.asarray(data, dtype=np.float64)
    if data.ndim == 3:
        return data[None], True
    if data.ndim == 4:
        return data, False
    raise ValueError(f"expected a 3D or (C, X, Y, Z) array, got shape {data.shape}")


def density_weight(density: np.ndarray) -> np.ndarray:
    """``density + eps``: the weight intensive properties are averaged by.

    The regularisation keeps a footprint containing no tissue at all well-defined - it
    falls back to the unweighted mean of the in-bounds taps rather than 0/0. It is scaled
    by the density's own magnitude because a ``density`` map need not be a [0, 1] float
    map: the spec allows any NIfTI numeric type.
    """
    density = np.asarray(density, dtype=np.float64)
    peak = float(density.max()) if density.size else 0.0
    eps = 1e-6 * peak if peak > 0.0 else 1.0
    return density + eps


def warn_grid_mismatch(name: str) -> None:
    warnings.warn(
        f"{name} does not share the density map's grid (see ../../NIFTI.md); resampling it "
        "unweighted, so its values may be diluted near edges",
        stacklevel=3,
    )
