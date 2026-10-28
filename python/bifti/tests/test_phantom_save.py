"""Saves loaded phantoms and checks the directory reloads to the same values, is
byte-for-byte reproducible, and shares bit-identical volumes."""

from __future__ import annotations

import copy
import gzip
from pathlib import Path

import nibabel
import numpy as np
import pytest

from bifti import BiftiPhantom, NiftiMapping, NiftiRef, NumpyPhantom

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
    assert not (tmp_path / "phantom_T1.nii.gz").exists()


def test_stack_layout_and_header(tmp_path):
    original = NumpyPhantom.load(DATA / "shapes.json")
    original.save(tmp_path)
    img = nibabel.load(tmp_path / "phantom_density.nii.gz")
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

    header = (a / "phantom_density.nii.gz").read_bytes()[:10]
    assert header[:2] == b"\x1f\x8b"
    assert header[3] == 0  # no file name
    assert header[4:8] == bytes(4)  # no mtime
    assert header[9] == 255  # OS unknown
    assert len(gzip.decompress((a / "phantom_density.nii.gz").read_bytes())) > 352


def test_identical_maps_share_one_volume(tmp_path):
    config = BiftiPhantom.load(DATA / "shapes.json")
    config.tissues["ring2"] = copy.deepcopy(config.tissues["ring"])
    phantom = NumpyPhantom.load_from_config(config, DATA)
    saved = BiftiPhantom.load(phantom.save(tmp_path))
    assert saved.tissues["ring"].dB0 == saved.tissues["ring2"].dB0
    assert saved.tissues["ring"].density == saved.tissues["ring2"].density
    assert nibabel.load(tmp_path / "phantom_dB0.nii.gz").shape[3] == 2  # disk + ring, not 3


def test_file_names_are_prefixed_with_the_phantom_name(tmp_path):
    NumpyPhantom.load(DATA / "shapes.json").save(tmp_path, "a.json")
    NumpyPhantom.load(DATA / "subj42-3T.json").save(tmp_path, "b.json")
    for name, prefix in (("shapes.json", "a"), ("subj42-3T.json", "b")):
        saved = NumpyPhantom.load(tmp_path / f"{prefix}.json")
        assert_same_phantom(NumpyPhantom.load(DATA / name), saved)
        config = BiftiPhantom.load(tmp_path / f"{prefix}.json")
        for tissue in config.tissues.values():
            assert tissue.density.file_name.name.startswith(f"{prefix}_")


def test_constant_threshold(tmp_path):
    config = BiftiPhantom.load(DATA / "shapes.json")
    phantom = NumpyPhantom.load_from_config(config, DATA)
    disk = phantom.tissues["disk"]
    disk.dB0 = np.full(disk.dB0.shape, 3.0)
    disk.dB0[0, 0, 0] += 1e-3  # std ~ 4e-6
    disk.B1_tx[1] = 1.0  # exactly constant
    std = disk.dB0.std()

    saved = BiftiPhantom.load(phantom.save(tmp_path / "off"))
    assert isinstance(saved.tissues["disk"].dB0, NiftiRef)
    assert isinstance(saved.tissues["disk"].B1_tx[1], NiftiRef)

    saved = BiftiPhantom.load(phantom.save(tmp_path / "zero", constant_threshold=0))
    assert isinstance(saved.tissues["disk"].dB0, NiftiRef)
    assert saved.tissues["disk"].B1_tx[1] == 1.0

    saved = BiftiPhantom.load(phantom.save(tmp_path / "on", constant_threshold=2 * std))
    assert saved.tissues["disk"].dB0 == pytest.approx(disk.dB0.mean())
    assert saved.tissues["disk"].B1_tx[1] == 1.0
    assert isinstance(saved.tissues["disk"].density, NiftiRef)  # never a constant
    assert isinstance(saved.tissues["ring"].dB0, NiftiRef)  # still varies
    assert not (tmp_path / "on" / "phantom_B1plus_1.nii.gz").exists()

    saved = BiftiPhantom.load(phantom.save(tmp_path / "neg", constant_threshold=-1))
    assert isinstance(saved.tissues["disk"].B1_tx[1], NiftiRef)


def write_complex_b1(path: Path) -> np.ndarray:
    """A complex two-channel B1 map on the grid of the shapes phantom."""
    density = nibabel.load(DATA / "shapes_density.nii.gz")
    x = np.linspace(0, np.pi, density.shape[0])[:, None, None, None]
    b1 = (np.exp(1j * x * np.array([1.0, -2.0])) * np.ones(density.shape[:3] + (2,)))
    nibabel.Nifti1Image(b1.astype(np.complex64), density.affine).to_filename(path)
    return b1


@pytest.mark.parametrize("name", ["shapes.json", "shapes_resliced.json"])
def test_complex_maps_round_trip(tmp_path, name):
    b1 = write_complex_b1(tmp_path / "b1.nii.gz")
    config = BiftiPhantom.load(DATA / name)
    disk = config.tissues["disk"]
    disk.B1_tx = [NiftiRef(tmp_path / "b1.nii.gz", 0), 1.0]
    disk.B1_rx = [NiftiMapping(NiftiRef(tmp_path / "b1.nii.gz", 1), "x * 2")]
    phantom = NumpyPhantom.load_from_config(config, DATA)

    loaded = phantom.tissues["disk"]
    assert np.iscomplexobj(loaded.B1_tx) and np.iscomplexobj(loaded.B1_rx)
    assert np.abs(loaded.B1_tx[0].imag).max() > 0.5
    if name == "shapes.json":
        np.testing.assert_allclose(loaded.B1_tx[0], b1[..., 0], atol=1e-6)
        np.testing.assert_allclose(loaded.B1_rx[0], 2 * b1[..., 1], atol=1e-6)

    out = tmp_path / "out"
    reloaded = NumpyPhantom.load(phantom.save(out))
    assert nibabel.load(out / "phantom_B1plus_0.nii.gz").get_data_dtype() == np.complex128
    assert_same_phantom(phantom, reloaded)


def test_complex_scalar_property_is_rejected(tmp_path):
    write_complex_b1(tmp_path / "b1.nii.gz")
    config = BiftiPhantom.load(DATA / "shapes.json")
    config.tissues["disk"].T1 = NiftiRef(tmp_path / "b1.nii.gz", 0)
    with pytest.raises(ValueError, match="T1 must be real-valued"):
        NumpyPhantom.load_from_config(config, DATA)
