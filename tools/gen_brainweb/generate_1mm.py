"""Fully automated pipeline that builds the collection-brainweb-1mm payload
from the published collection-brainweb Zenodo record.

collection-brainweb only ever reslices its native 0.5mm NIfTI data on load
(via `reslice_to`), so even its 1mm/2mm variants require downloading the full
0.5mm volumes. This script:

1. Looks up collection-brainweb's DOI in registry.json and downloads every
   file in that Zenodo record (NIfTIs + configs.tar), caching them so re-runs
   don't re-download.
2. Downsamples every NIfTI by a factor of 2 (averaging non-overlapping 2x2x2
   voxel blocks: 0.5mm -> 1mm), writing the result as float32 to keep files
   small.
3. Rewrites configs.tar: the native-0.5mm configs are dropped (that data no
   longer exists in this collection), the 1mm configs have their
   `reslice_to` removed (the downsampled data is now their native resolution),
   and the 2mm configs are left as-is (they still resample - now from the
   1mm data, since the NIfTI files they reference were overwritten in place).

Deps: requests, numpy, nibabel.

Usage:
    python generate_1mm.py <target_dir>

Writes into <target_dir>: every downsampled `*.nii.gz` plus the rewritten
`configs.tar`. Upload everything in <target_dir> (except `.download-cache/`)
to a single new Zenodo record - that's the whole payload for
collection-brainweb-1mm.

Also see `derive_registry_entry()` / `--print-registry-entry`, which builds
the collection-brainweb-1mm entry already added to registry.json (with a
placeholder DOI - replace it with the real one once the record is published).
"""

from __future__ import annotations

import argparse
import io
import json
import math
import re
import tarfile
from pathlib import Path
from urllib.parse import quote

import nibabel
import numpy as np
import requests

REPO_ROOT = Path(__file__).resolve().parents[2]
REGISTRY_JSON = REPO_ROOT / "registry.json"

# A Zenodo version DOI ("10.5281/zenodo.<id>") embeds the record id; see REGISTRY.md.
ZENODO_RECORD_URL = "https://zenodo.org/api/records/{record_id}"
ZENODO_FILE_URL = "https://zenodo.org/api/records/{record_id}/files/{filename}/content"

RESOLUTION_TAG_RE = re.compile(r"-(05mm|1mm|2mm)(?=-|\.json$)")

# Extent of the BrainWeb grid in mm (362x434x362 voxels at 0.5mm native
# resolution), duplicated from generate.py so this script has no import-time
# dependency on it (it lives in the same directory only by convention).
EXTENT_X = 181.0
EXTENT_Y = 217.0
EXTENT_Z = 181.0


def grid(extent: float, r: float) -> tuple[int, float]:
    """Voxel count and centered origin (mm) for a given extent/voxel size - same
    convention as generate.py's grid(), duplicated here (see note above) so the
    1mm affine can be computed directly from the phantom's physical size rather
    than derived from whatever affine the source 0.5mm file happens to store."""
    n = math.ceil(extent / r)
    origin = -(n - 1) / 2 * r
    return n, origin


def native_1mm_grid() -> tuple[tuple[int, int, int], np.ndarray]:
    """Resolution and centered 4x4 affine for the native 1mm grid."""
    (nx, ox), (ny, oy), (nz, oz) = (grid(EXTENT_X, 1.0), grid(EXTENT_Y, 1.0), grid(EXTENT_Z, 1.0))
    affine = np.array(
        [
            [1.0, 0.0, 0.0, ox],
            [0.0, 1.0, 0.0, oy],
            [0.0, 0.0, 1.0, oz],
            [0.0, 0.0, 0.0, 1.0],
        ]
    )
    return (nx, ny, nz), affine


# ===========================================================================
# Zenodo download
# ===========================================================================


def zenodo_record_id(doi: str) -> str:
    m = re.search(r"zenodo\.(\d+)$", doi)
    if not m:
        raise ValueError(f"not a Zenodo DOI: {doi!r}")
    return m.group(1)


def list_record_files(record_id: str) -> list[dict]:
    r = requests.get(ZENODO_RECORD_URL.format(record_id=record_id), timeout=60)
    r.raise_for_status()
    return r.json()["files"]


def download_record(record_id: str, cache_dir: Path) -> list[Path]:
    """Download every file in the Zenodo record into cache_dir (skips files
    already present with the expected size, so re-runs resume for free)."""
    cache_dir.mkdir(parents=True, exist_ok=True)
    paths = []
    for f in list_record_files(record_id):
        name = f["key"]
        size = f.get("size")
        dest = cache_dir / name
        if dest.exists() and (size is None or dest.stat().st_size == size):
            print(f"  cached    {name}")
        else:
            url = ZENODO_FILE_URL.format(record_id=record_id, filename=quote(name, safe=""))
            print(f"  download  {name} ...")
            with requests.get(url, stream=True, timeout=300) as resp:
                resp.raise_for_status()
                with open(dest, "wb") as fh:
                    for chunk in resp.iter_content(chunk_size=1 << 20):
                        fh.write(chunk)
        paths.append(dest)
    return paths


# ===========================================================================
# NIfTI downsampling
# ===========================================================================


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


def downsample_nifti(
    src: Path, dst: Path, target_resolution: tuple[int, int, int], target_affine: np.ndarray
) -> None:
    """Block-average one 0.5mm NIfTI by factor 2 and write it as float32,
    keeping the same filename (it now *is* the collection's native data).

    The output affine is the centered grid computed by native_1mm_grid() -
    the source file's own affine is not read or trusted, only its voxel data
    and shape.
    """
    img = nibabel.load(src)
    assert isinstance(img, nibabel.Nifti1Image), f"{src} is not a NIfTI-1 file"
    data = np.asarray(img.dataobj)
    assert data.ndim == 4, f"{src}: expected 4D data, got shape {data.shape}"

    averaged = block_average_2x2x2(data)
    if averaged.shape[:3] != tuple(target_resolution):
        raise ValueError(
            f"{src}: downsampled shape {averaged.shape[:3]} != expected "
            f"{tuple(target_resolution)} - is this really the native 0.5mm file?"
        )
    averaged = averaged.astype(np.float32)

    out_img = nibabel.Nifti1Image(averaged, target_affine)
    out_img.set_sform(target_affine, code=2)  # 2 == ALIGNED (subject space), see NIFTI.md
    out_img.set_qform(None, code=0)
    nibabel.save(out_img, dst)


# ===========================================================================
# configs.tar rewriting
# ===========================================================================


def resolution_tag(name: str) -> str:
    """'05mm' / '1mm' / '2mm' encoded in a phantom filename, e.g. subj04-3T-1mm-cor.json."""
    m = RESOLUTION_TAG_RE.search(name)
    if not m:
        raise ValueError(f"can't determine resolution tag from filename {name!r}")
    return m.group(1)


def rewrite_configs_tar(src_tar: Path, dst_tar: Path) -> list[str]:
    """Drop native-0.5mm configs, strip `reslice_to` from 1mm configs (now
    native), leave 2mm configs untouched (they still resample - now from the
    1mm data, since the NIfTI files they reference were overwritten in place).
    """
    kept: dict[str, bytes] = {}
    with tarfile.open(src_tar, "r") as tin:
        for member in tin.getmembers():
            if not member.isfile():
                continue
            tag = resolution_tag(member.name)
            if tag == "05mm":
                continue
            raw = tin.extractfile(member).read()
            if tag == "1mm":
                cfg = json.loads(raw)
                cfg.pop("reslice_to", None)
                raw = json.dumps(cfg, indent=2).encode("utf-8")
            kept[member.name] = raw

    with tarfile.open(dst_tar, "w") as tout:
        for name in sorted(kept):
            data = kept[name]
            info = tarfile.TarInfo(name=name)
            info.size = len(data)
            tout.addfile(info, io.BytesIO(data))

    return sorted(kept)


# ===========================================================================
# registry.json entry
# ===========================================================================

PLACEHOLDER_DOI = "10.5281/zenodo.00000000"


def _flatten(entries: list) -> list[str]:
    names = []
    for e in entries:
        if isinstance(e, str):
            names.append(e)
        else:
            names.extend(_flatten(e.get("phantoms", [])))
    return names


def _filter_out_native_05mm(entries: list) -> list:
    """Recursively drop every '-05mm' filename/group from a phantoms[] list."""
    out = []
    for e in entries:
        if isinstance(e, str):
            if resolution_tag(e) != "05mm":
                out.append(e)
            continue
        filtered_children = _filter_out_native_05mm(e.get("phantoms", []))
        if not filtered_children:
            continue
        new_group = dict(e)
        new_group["phantoms"] = filtered_children
        names = _flatten(filtered_children)
        if new_group.get("default") not in names:
            new_group["default"] = names[0]
        out.append(new_group)
    return out


def derive_registry_entry(
    registry: dict, source: str = "collection-brainweb", doi: str = PLACEHOLDER_DOI
) -> dict:
    """Build the collection-brainweb-1mm entry from the already-published
    collection-brainweb entry: same phantom filenames/groups, minus every
    native-0.5mm one."""
    src = registry[source]
    return {
        "description": (
            "1mm and 2mm variants of the 20 anatomical brain models from the BrainWeb "
            "Simulated Brain Database (McGill BIC; Aubert-Broche et al., IEEE TMI 2006) "
            "at 3T and 7T, downsampled from collection-brainweb's native 0.5mm data by "
            "averaging 2x2x2 voxel blocks (1mm is native resolution; 2mm is resampled "
            "from it on load). Same nested layout as collection-brainweb (subject > "
            "field strength/resolution), without the large native 0.5mm data."
        ),
        "keywords": [*src.get("keywords", []), "downsampled"],
        "authors": src["authors"],
        "license": src["license"],
        "doi": doi,
        "phantoms": _filter_out_native_05mm(src["phantoms"]),
    }


# ===========================================================================
# CLI
# ===========================================================================


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("target_dir", type=Path, nargs="?", help="where to write the collection-brainweb-1mm payload")
    parser.add_argument(
        "--download-cache", type=Path, default=None,
        help="directory to cache the downloaded collection-brainweb record in "
             "(default: <target_dir>/.download-cache)",
    )
    parser.add_argument(
        "--collection", default="collection-brainweb",
        help="registry.json collection to downsample from (default: collection-brainweb)",
    )
    parser.add_argument(
        "--print-registry-entry", action="store_true",
        help="print the collection-brainweb-1mm registry.json entry (no download) and exit",
    )
    args = parser.parse_args()

    registry = json.loads(REGISTRY_JSON.read_text(encoding="utf-8"))

    if args.print_registry_entry:
        print(json.dumps(derive_registry_entry(registry, args.collection), indent=2))
        return

    if args.target_dir is None:
        parser.error("target_dir is required unless --print-registry-entry is given")

    doi = registry[args.collection]["doi"]
    record_id = zenodo_record_id(doi)
    cache_dir = args.download_cache or (args.target_dir / ".download-cache")

    print(f"Downloading {args.collection} ({doi}) into {cache_dir} ...")
    downloaded = download_record(record_id, cache_dir)

    args.target_dir.mkdir(parents=True, exist_ok=True)
    target_resolution, target_affine = native_1mm_grid()

    nifti_files = [p for p in downloaded if p.name != "configs.tar"]
    print(f"\nDownsampling {len(nifti_files)} NIfTI file(s) by factor 2 ...")
    for src in sorted(nifti_files):
        downsample_nifti(src, args.target_dir / src.name, target_resolution, target_affine)
        print(f"  downsampled {src.name}")

    print("\nRewriting configs.tar ...")
    kept = rewrite_configs_tar(cache_dir / "configs.tar", args.target_dir / "configs.tar")
    n_dropped_by = len(kept)
    print(f"  kept {n_dropped_by} configs (dropped native-0.5mm, stripped reslice_to from 1mm)")

    print(f"\nDone. Upload every file in {args.target_dir} (except .download-cache/) "
          "to a single new Zenodo record for collection-brainweb-1mm.")


if __name__ == "__main__":
    main()
