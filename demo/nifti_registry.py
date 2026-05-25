# Fetch the public phantom registry and download phantoms from Zenodo. A readable
# reference for working off registry.json (see ../REGISTRY.md): list what's
# available, then pull a phantom's JSON plus every NIfTI it references into a
# local cache. Demo code - it assumes the registry is well-formed (CI validates
# it). Deps: requests. See nifti_phantom.py for the data model the JSON parses to.

from __future__ import annotations

import hashlib
import json
import re
from pathlib import Path

import requests

from nifti_phantom import NiftiPhantom, NiftiRef, NiftiMapping

HERE = Path(__file__).parent
CACHE = HERE / "cache"
REGISTRY_URL = (
    "https://raw.githubusercontent.com/mrx-org/nifti-phantoms/"
    "refs/heads/main/registry.json"
)
# A Zenodo version DOI ("10.5281/zenodo.<id>") embeds the record id; that's all
# we need to pull a file straight from the API.
ZENODO_FILE_URL = "https://zenodo.org/api/records/{record_id}/files/{filename}/content"


# ===========================================================================
# Public entry points
# ===========================================================================


def available_phantoms() -> dict[str, dict]:
    """Download the latest registry.json from GitHub and return it parsed.

    The raw bytes are cached as ``cache/registry-<hash>.json`` (one file per
    distinct version); the return value is the registry object as-is - a dict
    mapping each collection name to its entry (``doi``, ``phantoms``, etc.).
    """
    raw = _http_get(REGISTRY_URL)
    CACHE.mkdir(parents=True, exist_ok=True)
    cached = CACHE / f"registry-{hashlib.sha256(raw).hexdigest()[:12]}.json"
    if not cached.exists():
        cached.write_bytes(raw)
    return json.loads(raw)


def download_phantom(collection: str, name: str) -> Path:
    """Download a phantom's JSON and every NIfTI it references into the cache.

    Files go to ``cache/<collection>-<doi>/`` (the DOI's '/' replaced with '_').
    A version DOI is immutable, so anything already there is reused - re-runs
    download nothing. Returns the local phantom JSON path, ready for
    ``nifti_loader.load_phantom`` (the NIfTIs sit next to it).
    """
    doi = available_phantoms()[collection]["doi"]

    dir_ = CACHE / f"{collection}-{doi.replace('/', '_')}"
    dir_.mkdir(parents=True, exist_ok=True)

    json_path = _download_to(dir_, doi, name)
    phantom = NiftiPhantom.load(json_path)
    for filename in collect_nifti_files(phantom):
        _download_to(dir_, doi, filename)
    return json_path


# ===========================================================================
# Internals
# ===========================================================================


def _http_get(url: str) -> bytes:
    """GET a URL and return its bytes (raises on any non-2xx status)."""
    r = requests.get(url, timeout=60)
    r.raise_for_status()
    return r.content


def _download_to(dir_: Path, doi: str, filename: str) -> Path:
    """Download ``filename`` from the Zenodo record ``doi`` into ``dir_``.

    A no-op if the file is already cached (the DOI guarantees identical bytes).
    """
    dest = dir_ / filename
    if not dest.exists():
        record_id = re.search(r"zenodo\.(\d+)$", doi).group(1)
        url = ZENODO_FILE_URL.format(record_id=record_id, filename=filename)
        dest.write_bytes(_http_get(url))
    return dest


def _ref_file(prop) -> str | None:
    """The NIfTI filename a tissue property references, or None for a scalar."""
    if isinstance(prop, NiftiRef):
        return prop.file_name.name
    if isinstance(prop, NiftiMapping):
        return prop.file.file_name.name
    return None  # a plain number references no file


def collect_nifti_files(phantom: NiftiPhantom) -> list[str]:
    """Every distinct NIfTI filename referenced across all of a phantom's tissues."""
    files: list[str] = []
    seen: set[str] = set()

    def add(filename: str | None) -> None:
        if filename and filename not in seen:
            seen.add(filename)
            files.append(filename)

    for tissue in phantom.tissues.values():
        add(tissue.density.file_name.name)
        for prop in (tissue.T1, tissue.T2, tissue.T2dash, tissue.ADC, tissue.dB0):
            add(_ref_file(prop))
        for channel in (*tissue.B1_tx, *tissue.B1_rx):
            add(_ref_file(channel))
    return files


# ===========================================================================
# Example usage
# ===========================================================================

if __name__ == "__main__":
    for collection_name, entry in available_phantoms().items():
        print(f"{collection_name}  ({entry['doi']})")
        for phantom in entry["phantoms"]:
            print(f"    {phantom}")
