"""Loads the example phantoms, exercising the whole load path end to end."""

from __future__ import annotations

from pathlib import Path

import numpy as np
import pytest

from bifti import NumpyPhantom

DATA = Path(__file__).resolve().parent.parent / "examples" / "data"


@pytest.fixture(scope="module")
def native() -> NumpyPhantom:
    return NumpyPhantom.load(DATA / "shapes.json")


def test_native_phantom_keeps_its_own_grid(native):
    assert native.tissues
    for tissue in native.tissues.values():
        assert tissue.density.shape == (40, 32, 4)
        assert tissue.T1.shape == (40, 32, 4)
        assert tissue.B1_tx.shape[1:] == (40, 32, 4)


@pytest.mark.parametrize(
    ("name", "shape"),
    [("shapes_resliced.json", (60, 48, 4)), ("shapes_downsampled.json", (16, 12, 3))],
)
def test_resliced_phantoms_land_on_the_target_grid(name, shape):
    phantom = NumpyPhantom.load(DATA / name)
    for tissue in phantom.tissues.values():
        assert tissue.density.shape == shape
        assert tissue.T1.shape == shape
        assert tissue.dB0.shape == shape
        assert tissue.B1_tx.shape[1:] == shape
        assert np.isfinite(tissue.density).all()
        assert np.isfinite(tissue.T1).all()


@pytest.mark.parametrize(
    "name", ["shapes_resliced.json", "shapes_downsampled.json"]
)
def test_resampling_never_invents_values_outside_the_source_range(name, native):
    """The property density weighting buys.

    Averaging a map against the zeros outside the FOV - or between tissues - produces
    values below anything present in the source. A weighted average cannot: it is a convex
    combination of source values, so it stays inside their range.
    """
    phantom = NumpyPhantom.load(DATA / name)
    for tissue_name, tissue in phantom.tissues.items():
        for prop in ("T1", "T2", "dB0"):
            src = getattr(native.tissues[tissue_name], prop)
            got = getattr(tissue, prop)
            assert got.min() >= src.min() - 1e-6, f"{tissue_name}.{prop} undershoots"
            assert got.max() <= src.max() + 1e-6, f"{tissue_name}.{prop} overshoots"


def test_downsampling_conserves_total_tissue_mass(native):
    """Density is extensive, so summing it over the FOV (weighted by voxel volume) is
    invariant under resampling - the test that the old point-sampling kernel fails."""
    coarse = NumpyPhantom.load(DATA / "shapes_downsampled.json")

    for name, tissue in coarse.tissues.items():
        fine = native.tissues[name]
        # Voxel volumes from each grid's affine (both are axis-aligned here).
        fine_vol = np.prod([abs(fine.affine[i][i]) for i in range(3)])
        coarse_vol = np.prod([abs(tissue.affine[i][i]) for i in range(3)])

        assert tissue.density.sum() * coarse_vol == pytest.approx(
            fine.density.sum() * fine_vol, rel=0.02
        )


def test_downsampling_actually_averages(native):
    """A point-sampling kernel leaves the coarse map as noisy as the source.

    The background tissue is uniform 0.15 plus independent noise, and this fixture averages
    3x3x2 = 18 source voxels per output voxel, so its spread must fall by roughly sqrt(18).
    Measured over the *interior* only: the target FOV deliberately overhangs the source, so
    the edge voxels are partly empty and their spread reflects that, not the noise.
    """
    coarse = NumpyPhantom.load(DATA / "shapes_downsampled.json")
    fine = native.tissues["background"].density
    interior = coarse.tissues["background"].density[2:-2, 2:-2, 1:-1]

    assert interior.mean() == pytest.approx(fine.mean(), rel=0.02)
    assert interior.std() == pytest.approx(fine.std() / np.sqrt(18), rel=0.25)


def test_subj42_loads_with_channels_and_a_mapping():
    phantom = NumpyPhantom.load(DATA / "subj42-3T.json")
    for tissue in phantom.tissues.values():
        assert tissue.density.shape == (100, 100, 1)
        assert tissue.B1_tx.shape[1:] == (100, 100, 1)
        assert np.isfinite(tissue.dB0).all()
    assert phantom.tissues["gm"].B1_tx.shape[0] == 8
