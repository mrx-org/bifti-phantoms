"""Saves loaded phantoms and checks the directory reloads to the same values, is
byte-for-byte reproducible, and shares bit-identical volumes."""

from __future__ import annotations

import copy
import gzip
from pathlib import Path

import nibabel
import numpy as np
import pytest

from bifti import BiftiPhantom, NiftiMapping, NumpyPhantom

DATA = Path(__file__).resolve().parent.parent / "examples" / "data"


def assert_same_map(a: np.ndarray, b: np.ndarray, what: str):
    """Equal values; a constant stays a constant, so it may reload as a single voxel."""
    if b.size == 1 or a.size == 1:
        assert np.all(a == b.ravel()[0] if b.size == 1 else b == a.ravel()[0]), what
    else:
        np.testing.assert_array_equal(a, b, err_msg=what)


def assert_same_phantom(a: NumpyPhantom, b: NumpyPhantom):
    assert a.tissues.keys() == b.tissues.keys()
    for name, ta in a.tissues.items():
        tb = b.tissues[name]
        for field in ("density", "T1", "T2", "T2dash", "ADC", "dB0"):
            assert_same_map(getattr(ta, field), getattr(tb, field), f"{name} {field}")
        for field in ("B1_tx", "B1_rx"):
            va, vb = getattr(ta, field), getattr(tb, field)
            assert va.shape[0] == vb.shape[0], f"{name} {field} channels"
            for k in range(va.shape[0]):
                assert_same_map(va[k], vb[k], f"{name} {field}[{k}]")


def read_all(directory: Path) -> dict[str, bytes]:
    return {p.name: p.read_bytes() for p in sorted(directory.iterdir())}


@pytest.mark.parametrize(
    "name", ["shapes.json", "shapes_resliced.json", "subj42-3T.json"]
)
def test_round_trip(tmp_path, name):
    original = NumpyPhantom.load(DATA / name)
    json_path = original.save(tmp_path, "phantom.json")
    assert json_path == tmp_path / "phantom.json"

    saved = BiftiPhantom.load(json_path)
    assert saved.reslice_to is None
    for tissue in saved.tissues.values():  # func is baked in
        for prop in (tissue.T1, tissue.T2, tissue.T2dash, tissue.ADC, tissue.dB0):
            assert not isinstance(prop, NiftiMapping)
    assert_same_phantom(original, NumpyPhantom.load(json_path))


def test_constants_stay_constants(tmp_path):
    original = NumpyPhantom.load(DATA / "shapes.json")
    saved = BiftiPhantom.load(original.save(tmp_path))
    assert saved.tissues["disk"].T1 == 1.0
    assert not (tmp_path / "T1.nii.gz").exists()


def test_stack_layout_and_header(tmp_path):
    original = NumpyPhantom.load(DATA / "shapes.json")
    original.save(tmp_path)
    img = nibabel.load(tmp_path / "density.nii.gz")
    assert img.shape == (40, 32, 4, 3)
    assert int(img.header["sform_code"]) == 2
    assert int(img.header["qform_code"]) == 0
    np.testing.assert_allclose(
        img.get_sform()[:3], original.tissues["disk"].affine, rtol=1e-6
    )
    # tissues are stacked in sorted-name order: background, disk, ring
    np.testing.assert_array_equal(
        np.asarray(img.dataobj)[..., 1], original.tissues["disk"].density
    )


def test_output_is_deterministic(tmp_path):
    a, b = tmp_path / "a", tmp_path / "b"
    NumpyPhantom.load(DATA / "shapes_resliced.json").save(a)
    # a different tissue insertion order must not change the output
    config = BiftiPhantom.load(DATA / "shapes_resliced.json")
    config.tissues = dict(reversed(list(config.tissues.items())))
    NumpyPhantom.load_from_config(config, DATA).save(b)
    assert read_all(a) == read_all(b)

    header = (a / "density.nii.gz").read_bytes()[:10]
    assert header[:2] == b"\x1f\x8b"
    assert header[3] == 0  # no file name
    assert header[4:8] == bytes(4)  # no mtime
    assert header[9] == 255  # OS unknown
    assert len(gzip.decompress((a / "density.nii.gz").read_bytes())) > 352


def test_identical_maps_share_one_volume(tmp_path):
    config = BiftiPhantom.load(DATA / "shapes.json")
    config.tissues["ring2"] = copy.deepcopy(config.tissues["ring"])
    phantom = NumpyPhantom.load_from_config(config, DATA)
    saved = BiftiPhantom.load(phantom.save(tmp_path))
    assert saved.tissues["ring"].dB0 == saved.tissues["ring2"].dB0
    assert saved.tissues["ring"].density == saved.tissues["ring2"].density
    assert nibabel.load(tmp_path / "dB0.nii.gz").shape[3] == 2  # disk + ring, not 3
