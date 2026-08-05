"""Downsample the native 0.5mm BrainWeb NIfTI files (from the collection-brainweb
Zenodo record) to 1mm by averaging non-overlapping 2x2x2 voxel blocks, and
generate the matching phantom JSON configs for a new collection-brainweb-1mm.

Unlike generate.py (which only ever reslices the 0.5mm data on load via
`reslice_to`, so every resolution still requires downloading the full 0.5mm
volumes), this script writes real, smaller 1mm NIfTI files. The 1mm resolution
is exact block-averaging (no interpolation); the 2mm variant is still a
`reslice_to` config that resamples the 1mm files on load, exactly like
generate.py does for its 1mm/2mm variants - so 2mm needs no separate data.

Usage:
    python generate_1mm.py <src_dir> <out_dir>

<src_dir> must contain the native 0.5mm NIfTI triplet for every subject in
SUBJECTS, as downloaded from the collection-brainweb Zenodo record:
    subjXX.nii.gz, subjXX_dB0.nii.gz, subjXX_B1+.nii.gz

Writes into <out_dir>:
    subjXX-1mm.nii.gz, subjXX-1mm_dB0.nii.gz, subjXX-1mm_B1+.nii.gz  (float32)
    configs.tar  (every subjXX-{3T,7T}-{1mm,2mm}[-tra/cor/sag].json)

Upload every file in <out_dir> to a single new Zenodo record - that record is
the whole payload for the collection-brainweb-1mm registry entry.
"""

from __future__ import annotations

import argparse
import io
import json
import sys
import tarfile
from pathlib import Path

import nibabel
import numpy as np

sys.path.insert(0, str(Path(__file__).resolve().parent))
from generate import (  # noqa: E402 - reuse the exact conventions collection-brainweb uses
    SUBJECTS,
    reslice_3d,
    reslice_tra,
    reslice_cor,
    reslice_sag,
    build_config,
    _replace_subject,
)

RESOLUTIONS = [("1mm", 1.0), ("2mm", 2.0)]

NIFTI_SUFFIXES = ["", "_dB0", "_B1+"]


def block_average_2x2x2(data: np.ndarray) -> np.ndarray:
    """Average non-overlapping 2x2x2 blocks along the first 3 axes of a 4D array."""
    nx, ny, nz, nt = data.shape
    if nx % 2 or ny % 2 or nz % 2:
        raise ValueError(
            f"native shape {data.shape[:3]} is not evenly divisible by 2 in every "
            "spatial dimension - cannot block-average 2x2x2"
        )
    data = data.astype(np.float64, copy=False)
    reshaped = data.reshape(nx // 2, 2, ny // 2, 2, nz // 2, 2, nt)
    return reshaped.mean(axis=(1, 3, 5))


def downsample_nifti(src: Path, dst: Path, expected_resolution: tuple[int, int, int]) -> None:
    """Block-average one 0.5mm NIfTI to 1mm and write it as float32."""
    img = nibabel.load(src)
    assert isinstance(img, nibabel.Nifti1Image), f"{src} is not a NIfTI-1 file"
    data = np.asarray(img.dataobj)
    assert data.ndim == 4, f"{src}: expected 4D data, got shape {data.shape}"

    sform = img.get_sform()
    off_diag = sform[:3, :3] - np.diag(np.diagonal(sform[:3, :3]))
    assert np.allclose(off_diag, 0), (
        f"{src}: affine has off-diagonal terms - phantom NIfTIs must be axis-aligned "
        "RAS+ (see ../../NIFTI.md)"
    )

    averaged = block_average_2x2x2(data)
    if averaged.shape[:3] != tuple(expected_resolution):
        raise ValueError(
            f"{src}: downsampled shape {averaged.shape[:3]} != expected "
            f"{tuple(expected_resolution)} - is this really the native 0.5mm file?"
        )
    averaged = averaged.astype(np.float32)

    # Averaging source voxels (2k, 2k+1) along an axis lands the result at their
    # midpoint: new voxel size doubles, new origin = old origin + half the old
    # voxel size. This reproduces exactly the grid generate.py's reslice_3d(1.0)
    # computes for the 1mm reslice_to variant, so "native" 1mm here is
    # geometrically identical to that resampled grid.
    new_affine = sform.copy()
    for ax in range(3):
        old_r = sform[ax, ax]
        new_affine[ax, ax] = old_r * 2.0
        new_affine[ax, 3] = sform[ax, 3] + 0.5 * old_r

    out_img = nibabel.Nifti1Image(averaged, new_affine)
    out_img.set_sform(new_affine, code=2)  # 2 == ALIGNED (subject space), see NIFTI.md
    out_img.set_qform(None, code=0)
    nibabel.save(out_img, dst)


def load_1mm_template() -> dict:
    """subj04.json with its NIfTI references pointed at the new subj04-1mm* files."""
    with open(Path(__file__).resolve().parent / "subj04.json") as f:
        template = json.load(f)
    for old, new in [
        ("subj04.nii.gz", "subj04-1mm.nii.gz"),
        ("subj04_dB0.nii.gz", "subj04-1mm_dB0.nii.gz"),
        ("subj04_B1+.nii.gz", "subj04-1mm_B1+.nii.gz"),
    ]:
        template = _replace_subject(template, old, new)
    return template


def build_configs() -> dict[str, dict]:
    template = load_1mm_template()
    configs = {}

    for subj in SUBJECTS:
        for field, field_str in [(3, "3T"), (7, "7T")]:
            for res_str, r in RESOLUTIONS:
                prefix = f"subj{subj:02d}-{field_str}-{res_str}"

                # 3D: no reslice_to at native 1mm, downsample (reslice) otherwise.
                reslice_3d_val = None if r == 1.0 else reslice_3d(r)
                configs[f"{prefix}.json"] = build_config(template, subj, field, reslice_3d_val)

                for orient, fn in [("tra", reslice_tra), ("cor", reslice_cor), ("sag", reslice_sag)]:
                    configs[f"{prefix}-{orient}.json"] = build_config(template, subj, field, fn(r))

    return configs


def write_configs_tar(configs: dict[str, dict], out_dir: Path) -> None:
    with tarfile.open(out_dir / "configs.tar", "w") as tar:
        for name in sorted(configs):
            data = json.dumps(configs[name], indent=2).encode("utf-8")
            info = tarfile.TarInfo(name=name)
            info.size = len(data)
            tar.addfile(info, io.BytesIO(data))


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("src_dir", type=Path, help="directory with the native 0.5mm subjXX*.nii.gz files")
    parser.add_argument("out_dir", type=Path, help="directory to write the 1mm NIfTIs and configs.tar into")
    args = parser.parse_args()

    args.out_dir.mkdir(parents=True, exist_ok=True)
    expected_resolution = tuple(reslice_3d(1.0)["resolution"])

    for subj in SUBJECTS:
        subj_str = f"subj{subj:02d}"
        for suffix in NIFTI_SUFFIXES:
            src = args.src_dir / f"{subj_str}{suffix}.nii.gz"
            dst = args.out_dir / f"{subj_str}-1mm{suffix}.nii.gz"
            downsample_nifti(src, dst, expected_resolution)
        print(f"downsampled {subj_str}")

    configs = build_configs()
    write_configs_tar(configs, args.out_dir)

    n_nifti = len(SUBJECTS) * len(NIFTI_SUFFIXES)
    print(f"\nWrote {n_nifti} downsampled NIfTI file(s) and configs.tar "
          f"({len(configs)} configs) to {args.out_dir}")
    print("Upload every *.nii.gz file plus configs.tar in that directory to a single "
          "new Zenodo record for collection-brainweb-1mm.")


if __name__ == "__main__":
    main()
