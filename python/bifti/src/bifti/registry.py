# Fetch the public phantom registry and download phantoms from Zenodo. A readable
# reference for working off registry.json (see ../REGISTRY.md): list what's
# available, then pull a phantom's JSON plus every NIfTI it references into a
# local cache. Demo code - it assumes the registry is well-formed (CI validates
# it). Deps: requests. See nifti_phantom.py for the data model the JSON parses to.

from __future__ import annotations

import io
import json
import re
import tarfile
from pathlib import Path
from urllib.parse import quote

import requests

from .phantom import BiftiPhantom, NiftiRef, NiftiMapping

HERE = Path(__file__).parent
CACHE = HERE / "cache"
REGISTRY_URL = (
    "https://raw.githubusercontent.com/mrx-org/bifti-phantoms/"
    "refs/heads/main/registry.json"
)
# A Zenodo version DOI ("10.5281/zenodo.<id>") embeds the record id; that's all
# we need to pull a file straight from the API.
ZENODO_FILE_URL = "https://zenodo.org/api/records/{record_id}/files/{filename}/content"


# ===========================================================================
# Public entry points
# ===========================================================================


def load_registry():
    """Download the latest registry.json from GitHub and return it parsed."""
    return json.loads(_http_get(REGISTRY_URL))


def load_registry_phantom(collection: str, name: str) -> Path:
    """Download a phantom's JSON and every NIfTI it references into the cache.

    Returns the path to the .json of the downloaded phantom. Re-running this
    function does nothing as phantoms are immutable and cached.
    """
    doi = load_registry()[collection]["doi"]

    dir_ = CACHE / f"{collection}-{doi.replace('/', '_')}"
    dir_.mkdir(parents=True, exist_ok=True)

    record_id = _zenodo_record_id(doi)
    json_path = _download_json(dir_, record_id, name)
    phantom = BiftiPhantom.load(json_path)
    for filename in collect_nifti_files(phantom):
        _download_to(dir_, doi, filename)
    return json_path


# ===========================================================================
# Internals
# ===========================================================================


def _zenodo_record_id(doi: str) -> str:
    """Extract the record id from a Zenodo version DOI ("10.5281/zenodo.<id>")."""
    m = re.search(r"zenodo\.(\d+)$", doi)
    if not m:
        raise ValueError(f"Not a Zenodo DOI: {doi!r}")
    return m.group(1)


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
        record_id = _zenodo_record_id(doi)
        url = ZENODO_FILE_URL.format(record_id=record_id, filename=filename)
        dest.write_bytes(_http_get(url))
    return dest


def _download_json(dir_: Path, record_id: str, name: str) -> Path:
    """Resolve a phantom JSON following the spec lookup order (REGISTRY.md).

    1. Try fetching ``name`` directly from the Zenodo record.
    2. Fall back to downloading ``configs.tar`` and extracting ``name`` from it.

    ``configs.tar`` is cached to ``dir_/configs.tar`` so multiple calls for
    phantoms in the same collection only download the archive once.
    """
    dest = dir_ / name
    if dest.exists():
        return dest

    # 1. Direct
    try:
        url = ZENODO_FILE_URL.format(record_id=record_id, filename=quote(name, safe=""))
        dest.write_bytes(_http_get(url))
        return dest
    except Exception:
        pass

    # 2. configs.tar
    archive = dir_ / "configs.tar"
    if not archive.exists():
        url = ZENODO_FILE_URL.format(record_id=record_id, filename="configs.tar")
        archive.write_bytes(_http_get(url))

    with tarfile.open(fileobj=io.BytesIO(archive.read_bytes()), mode="r:") as tf:
        member = tf.getmember(name)  # raises KeyError if absent
        extracted = tf.extractfile(member)
        if extracted is None:
            raise ValueError(f"{name!r} is not a regular file in configs.tar")
        dest.write_bytes(extracted.read())
    return dest


def _ref_file(prop) -> str | None:
    """The NIfTI filename a tissue property references, or None for a scalar."""
    if isinstance(prop, NiftiRef):
        return prop.file_name.name
    if isinstance(prop, NiftiMapping):
        return prop.file.file_name.name
    return None  # a plain number references no file


def collect_nifti_files(phantom: BiftiPhantom) -> list[str]:
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
    for collection_name, entry in load_registry().items():
        print(f"{collection_name}  ({entry['doi']})")
        for phantom in entry["phantoms"]:
            print(f"    {phantom}")
