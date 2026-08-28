"""Unit tests for the density-weighted footprint resampler."""

from __future__ import annotations

import numpy as np
import pytest

from bifti._backend import Backend, get_backend
from bifti.resample import Resampler, density_weight


def affine(spacing, origin) -> list[list[float]]:
    return [
        [spacing[0], 0.0, 0.0, origin[0]],
        [0.0, spacing[1], 0.0, origin[1]],
        [0.0, 0.0, spacing[2], origin[2]],
    ]


def grids(n: int, factor: float):
    """Two grids over the same FOV: source at spacing 1, target `factor` times coarser."""
    shift = (factor - 1.0) / 2.0
    return (
        affine([1, 1, 1], [0, 0, 0]),
        (n, n, n),
        affine([factor] * 3, [shift] * 3),
        [int(n / factor)] * 3,
    )


def plain(r: Resampler, data: np.ndarray) -> np.ndarray:
    """Resample with a uniform density, so the weighting is a no-op."""
    w = np.ones(data.shape[-3:], dtype=np.float64)
    return r.resample_weighted(data, w, r.weight_sum(w))


@pytest.mark.parametrize("factor", [2, 4])
def test_downsample_is_an_exact_block_mean(factor):
    src_affine, src_shape, dst_affine, dst_shape = grids(8, factor)
    data = np.arange(np.prod(src_shape), dtype=np.float64).reshape(src_shape)

    r = Resampler.build(src_affine, src_shape, dst_affine, dst_shape)
    assert r.kind == "separable"
    out = plain(r, data)

    # The exact block mean, computed independently by reshaping.
    k = factor
    want = data.reshape(8 // k, k, 8 // k, k, 8 // k, k).mean(axis=(1, 3, 5))
    np.testing.assert_allclose(out, want, atol=1e-12)


def test_t1_is_not_diluted_at_the_fov_edge():
    """The bug this module exists to fix.

    An output voxel straddling the edge of the source FOV must not average the tissue's T1
    against the zeros outside it.
    """
    src_affine, src_shape = affine([1, 1, 1], [0, 0, 0]), (8, 8, 8)
    density = np.ones(src_shape)
    t1 = np.full(src_shape, 1.5)

    # A 4x coarser target covering *more* than the source, so the outer output voxels are
    # only partly filled.
    r = Resampler.build(src_affine, src_shape, affine([4, 4, 4], [-2, -2, -2]), [4, 4, 4])

    weight = density_weight(density)
    out_t1 = r.resample_weighted(t1, weight, r.weight_sum(weight))
    out_density = r.resample_plain(density)

    # Every voxel holding any tissue reports the tissue's true T1 ...
    filled = out_density > 1e-6
    np.testing.assert_allclose(out_t1[filled], 1.5, atol=1e-6)
    # ... while density *does* fall off at the edge, which is physically correct: those
    # voxels really are only partly filled with tissue.
    assert out_density.max() > 0.99
    assert ((out_density > 0) & (out_density < 0.9)).any()


def test_plain_average_would_dilute_t1():
    """Guards the test above: an unweighted average really does show the bug."""
    src_affine, src_shape = affine([1, 1, 1], [0, 0, 0]), (8, 8, 8)
    t1 = np.full(src_shape, 1.5)
    r = Resampler.build(src_affine, src_shape, affine([4, 4, 4], [-2, -2, -2]), [4, 4, 4])
    assert r.resample_plain(t1).min() < 1.4


def test_empty_density_footprint_falls_back_to_plain_mean():
    """A footprint with no tissue has no density to weight by; it must not become NaN."""
    src_affine, src_shape, dst_affine, dst_shape = grids(4, 2)
    values = np.full(src_shape, 3.25)

    r = Resampler.build(src_affine, src_shape, dst_affine, dst_shape)
    weight = density_weight(np.zeros(src_shape))
    out = r.resample_weighted(values, weight, r.weight_sum(weight))

    assert np.isfinite(out).all()
    np.testing.assert_allclose(out, 3.25, atol=1e-9)


def test_upsample_matches_trilinear():
    """Not downsampling must behave exactly as plain trilinear interpolation did."""
    scipy_ndimage = pytest.importorskip("scipy.ndimage")

    src_affine, src_shape = affine([2, 2, 2], [0, 0, 0]), (4, 4, 4)
    rng = np.random.default_rng(0)
    data = rng.random(src_shape)

    dst_affine, dst_shape = affine([1, 1, 1], [0, 0, 0]), [7, 7, 7]
    r = Resampler.build(src_affine, src_shape, dst_affine, dst_shape)
    out = plain(r, data)

    # Reference: scipy's own order-1 (trilinear) sampling at the same coordinates.
    m = np.linalg.inv(np.vstack([src_affine, [0, 0, 0, 1]])) @ np.vstack(
        [dst_affine, [0, 0, 0, 1]]
    )
    want = scipy_ndimage.affine_transform(
        data, m[:3, :3], offset=m[:3, 3], output_shape=tuple(dst_shape), order=1, cval=0.0
    )
    np.testing.assert_allclose(out, want, atol=1e-9)


def test_identity_grid_is_an_exact_identity():
    a, shape = affine([1.5, 2.0, 3.0], [-10, 4, 0.5]), (3, 4, 5)
    data = np.arange(np.prod(shape), dtype=np.float64).reshape(shape) * 0.37

    r = Resampler.build(a, shape, a, list(shape))
    assert r.is_identity
    np.testing.assert_array_equal(plain(r, data), data)


def test_permuted_axes_are_resampled_into_output_order():
    """An affine mapping output x onto source z still separates - via a permutation."""
    src_affine, src_shape = affine([1, 1, 1], [0, 0, 0]), (2, 3, 4)
    data = np.arange(24, dtype=np.float64).reshape(src_shape)

    # Output axis 0 -> source axis 2, 1 -> 1, 2 -> 0.
    dst_affine = [[0, 0, 1, 0], [0, 1, 0, 0], [1, 0, 0, 0]]
    r = Resampler.build(src_affine, src_shape, dst_affine, [4, 3, 2])
    assert r.kind == "separable"
    np.testing.assert_allclose(plain(r, data), np.transpose(data, (2, 1, 0)), atol=1e-9)


def test_a_voxel_that_only_touches_the_source_gets_no_weight():
    """An output voxel with zero-width overlap must end up with exactly zero weight.

    Inverting the source affine is inexact, so such a voxel can pick up a ~1e-17 weight.
    Divided by an equally tiny total weight, that reports the single touched voxel's value
    as the footprint average, producing a hard non-zero fringe just outside the FOV. This
    is the geometry of the `shapes_downsampled` fixture, which hits it.
    """
    src_affine, src_shape = affine([3, 3, 5], [-60, -48, -10]), (40, 32, 4)
    dst_affine, dst_shape = affine([9, 9, 10], [-66, -51, -12.5]), [16, 12, 3]

    r = Resampler.build(src_affine, src_shape, dst_affine, dst_shape)
    ws = r.weight_sum(np.ones(src_shape))
    # Output x = 0 and x = 15 lie entirely outside the source along x.
    assert (ws[0] == 0).all()
    assert (ws[15] == 0).all()

    data = np.arange(np.prod(src_shape), dtype=np.float64).reshape(src_shape) + 1.0
    out = r.resample_weighted(data, np.ones(src_shape), ws)
    assert (out[0] == 0).all()
    assert (out[15] == 0).all()


def test_oblique_affine_uses_the_quadrature_path():
    src_affine, src_shape = affine([1, 1, 1], [0, 0, 0]), (8, 8, 8)
    c, s = 0.8, 0.6  # a 37 degree rotation about z
    dst_affine = [[2 * c, -2 * s, 0, 1], [2 * s, 2 * c, 0, 1], [0, 0, 2, 0.5]]

    r = Resampler.build(src_affine, src_shape, dst_affine, [4, 4, 4])
    assert r.kind == "oblique"

    # A constant field survives resampling wherever it is fully covered, and is never
    # amplified beyond its source value.
    out = plain(r, np.full(src_shape, 2.5))
    assert np.isfinite(out).all()
    assert np.isclose(out.max(), 2.5, atol=1e-9)
    assert (out <= 2.5 + 1e-9).all()


def test_channels_resample_together():
    """All coil channels of a B1 map go through one contraction, matching per-channel."""
    src_affine, src_shape, dst_affine, dst_shape = grids(4, 2)
    rng = np.random.default_rng(1)
    stack = rng.random((3, *src_shape))

    r = Resampler.build(src_affine, src_shape, dst_affine, dst_shape)
    w = np.ones(src_shape)
    ws = r.weight_sum(w)

    together = r.resample_weighted(stack, w, ws)
    assert together.shape == (3, *dst_shape)
    for i in range(3):
        np.testing.assert_allclose(
            together[i], r.resample_weighted(stack[i], w, ws), atol=1e-12
        )


def test_numpy_and_torch_backends_agree():
    pytest.importorskip("torch")
    src_affine, src_shape, dst_affine, dst_shape = grids(8, 2)
    rng = np.random.default_rng(2)
    data = rng.random(src_shape)
    density = rng.random(src_shape)

    results = []
    for name in ("numpy", "torch"):
        r = Resampler.build(
            src_affine, src_shape, dst_affine, dst_shape, backend=get_backend(name)
        )
        weight = density_weight(density)
        results.append(r.resample_weighted(data, weight, r.weight_sum(weight)))
    # float32 on CUDA, float64 on CPU - so allow the looser of the two.
    np.testing.assert_allclose(results[0], results[1], rtol=1e-5, atol=1e-6)


def test_backend_selection():
    assert isinstance(get_backend("numpy"), Backend)
    with pytest.raises(ValueError):
        get_backend("nonsense")
