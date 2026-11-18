"""``reslice_to.move_to``: placing the resampled grid independently of the sampling."""

from __future__ import annotations

import copy
from pathlib import Path

import numpy as np
import pytest

from bifti import BiftiPhantom, NumpyPhantom
from bifti.phantom import ResliceTo

DATA = Path(__file__).resolve().parent.parent / "examples" / "data"

SAMPLED = [[3.0, 0, 0, -50], [0, 2.0, 0, 7], [0, 0, 5.0, 12]]


def voxel_positions(affine, resolution) -> np.ndarray:
    idx = np.stack(np.meshgrid(*[np.arange(n) for n in resolution], indexing="ij"), -1)
    A = np.asarray(affine, dtype=float)
    return idx @ A[:, :3].T + A[:, 3]


def test_without_move_to_the_grid_stays_where_it_was_sampled():
    r = ResliceTo(affine=SAMPLED, resolution=[4, 5, 1])
    assert r.placed_affine() == SAMPLED
    assert "move_to" not in r.to_dict()


@pytest.mark.parametrize("n", [4, 5])
def test_center_puts_voxel_n_div_2_at_the_origin(n):
    r = ResliceTo(affine=SAMPLED, resolution=[n, n, 1], move_to="center")
    placed = np.asarray(r.placed_affine())
    np.testing.assert_allclose(placed[:, :3], np.asarray(SAMPLED)[:, :3])

    x = voxel_positions(placed, [n, n, 1])[:, 0, 0, 0] / 3.0
    # even: -n/2 .. n/2-1, odd: -(n-1)/2 .. (n-1)/2
    np.testing.assert_allclose(x, np.arange(n) - n // 2)


def test_center_keeps_rotation():
    c, s = 0.8, 0.6
    affine = [[2 * c, -2 * s, 0, 10], [2 * s, 2 * c, 0, -4], [0, 0, 3, 1]]
    r = ResliceTo(affine=affine, resolution=[6, 4, 2], move_to="center")
    pos = voxel_positions(r.placed_affine(), [6, 4, 2])
    np.testing.assert_allclose(pos[3, 2, 1], 0, atol=1e-12)


def test_explicit_move_to_is_used_as_given_and_round_trips():
    target = np.array([[3.0, 0, 0, 1], [0, 2.0, 0, 2], [0, 0, 5.0, 3], [0, 0, 0, 1]])
    r = ResliceTo(affine=SAMPLED, resolution=[4, 5, 1], move_to=target)
    assert r.placed_affine() == target[:3].tolist()

    back = ResliceTo.from_dict(r.to_dict())
    assert back == r
    assert ResliceTo.from_dict({**r.to_dict(), "move_to": "center"}).move_to == "center"


def test_invalid_move_to_is_rejected():
    with pytest.raises(ValueError):
        ResliceTo(affine=SAMPLED, resolution=[4, 5, 1], move_to="middle")
    with pytest.raises(ValueError):
        ResliceTo(affine=SAMPLED, resolution=[4, 5, 1], move_to=[[1, 0, 0], [0, 1, 0]])


def test_different_voxel_size_warns():
    with pytest.warns(UserWarning, match="stretched"):
        ResliceTo(
            affine=SAMPLED,
            resolution=[4, 5, 1],
            move_to=[[1.0, 0, 0, 0], [0, 2.0, 0, 0], [0, 0, 5.0, 0]],
        )


def test_move_to_changes_placement_not_sampling():
    config = BiftiPhantom.load(DATA / "shapes_resliced.json")
    plain = NumpyPhantom.load_from_config(config, DATA)

    moved_config = copy.deepcopy(config)
    moved_config.reslice_to.move_to = "center"
    moved = NumpyPhantom.load_from_config(moved_config, DATA)

    for name, tissue in moved.tissues.items():
        np.testing.assert_array_equal(tissue.density, plain.tissues[name].density)
        np.testing.assert_array_equal(tissue.T1, plain.tissues[name].T1)
        assert tissue.affine == moved_config.reslice_to.placed_affine()
        assert tissue.resliced.affine == config.reslice_to.affine


def test_saved_phantom_keeps_the_placed_affine(tmp_path):
    config = BiftiPhantom.load(DATA / "shapes_resliced.json")
    config.reslice_to.move_to = "center"
    json_path = NumpyPhantom.load_from_config(config, DATA).save(tmp_path)

    reloaded = NumpyPhantom.load(json_path)
    for tissue in reloaded.tissues.values():
        np.testing.assert_allclose(
            np.asarray(tissue.affine)[:3], config.reslice_to.placed_affine(), atol=1e-4
        )
