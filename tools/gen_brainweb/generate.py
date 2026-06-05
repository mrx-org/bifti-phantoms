"""Code used to generate BrainWeb config files used in https://doi.org/10.5281/zenodo.20396886"""

import copy
import io
import json
import math
import tarfile

SUBJECTS = [4, 5, 6, 18, 20, 38, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 51, 52, 53, 54]

TISSUE_PARAMS = {
    3: {
        "gm":      {"T1": 1.56,  "T2": 0.083, "T2'": 0.32,   "ADC": 0.83},
        "wm":      {"T1": 0.83,  "T2": 0.075, "T2'": 0.18,   "ADC": 0.65},
        "csf":     {"T1": 4.16,  "T2": 1.65,  "T2'": 0.059,  "ADC": 3.19},
        "vessels": {"T1": 4.16,  "T2": 1.65,  "T2'": 0.059,  "ADC": 3.19},
        "fat":     {"T1": 0.37,  "T2": 0.125, "T2'": 0.012,  "ADC": 0.1},
    },
    7: {
        "gm":      {"T1": 1.67,  "T2": 0.043, "T2'": 0.82,   "ADC": 0.83},
        "wm":      {"T1": 1.22,  "T2": 0.037, "T2'": 0.65,   "ADC": 0.65},
        "csf":     {"T1": 4.0,   "T2": 0.8,   "T2'": 0.204,  "ADC": 3.19},
        "vessels": {"T1": 4.0,   "T2": 0.8,   "T2'": 0.204,  "ADC": 3.19},
        "fat":     {"T1": 0.374, "T2": 0.125, "T2'": 0.0117, "ADC": 0.1},
    },
}

FAT_DB0_FUNC = {3: "x - 440", 7: "x - 1020"}

# Physical extents in mm: 362×434×362 voxels at 0.5mm native resolution
EXTENT_X = 181.0
EXTENT_Y = 217.0
EXTENT_Z = 181.0
SLICE_THICKNESS = 8.0

RESOLUTIONS = [("05mm", 0.5), ("1mm", 1.0), ("2mm", 2.0)]


def _num(x):
    """Return int if value is a whole number, else float."""
    if isinstance(x, float) and x == int(x):
        return int(x)
    return x


def grid(extent, r):
    """Number of voxels and first-voxel origin (mm) for a given voxel size."""
    n = math.ceil(extent / r)
    origin = _num(-(n - 1) / 2 * r)
    return n, origin


def reslice_3d(r):
    nx, ox = grid(EXTENT_X, r)
    ny, oy = grid(EXTENT_Y, r)
    nz, oz = grid(EXTENT_Z, r)
    return {
        "affine": [[_num(r), 0, 0, ox], [0, _num(r), 0, oy], [0, 0, _num(r), oz]],
        "resolution": [nx, ny, nz],
    }


def reslice_tra(r):
    """Transversal: columns=X, rows=Y, slice=Z."""
    nx, ox = grid(EXTENT_X, r)
    ny, oy = grid(EXTENT_Y, r)
    return {
        "affine": [[_num(r), 0, 0, ox], [0, _num(r), 0, oy], [0, 0, SLICE_THICKNESS, 0]],
        "resolution": [nx, ny, 1],
    }


def reslice_cor(r):
    """Coronal: columns=X, rows=Z, slice=Y."""
    nx, ox = grid(EXTENT_X, r)
    nz, oz = grid(EXTENT_Z, r)
    return {
        "affine": [[_num(r), 0, 0, ox], [0, 0, SLICE_THICKNESS, 0], [0, _num(r), 0, oz]],
        "resolution": [nx, nz, 1],
    }


def reslice_sag(r):
    """Sagittal: columns=Y, rows=Z, slice=X."""
    ny, oy = grid(EXTENT_Y, r)
    nz, oz = grid(EXTENT_Z, r)
    return {
        "affine": [[0, 0, SLICE_THICKNESS, 0], [_num(r), 0, 0, oy], [0, _num(r), 0, oz]],
        "resolution": [ny, nz, 1],
    }


def _replace_subject(val, src, dst):
    if isinstance(val, str):
        return val.replace(src, dst)
    if isinstance(val, list):
        return [_replace_subject(v, src, dst) for v in val]
    if isinstance(val, dict):
        return {k: _replace_subject(v, src, dst) for k, v in val.items()}
    return val


def build_config(template, subj, field, reslice):
    cfg = copy.deepcopy(template)
    cfg["system"]["B0"] = field

    subj_str = f"subj{subj:02d}"
    params = TISSUE_PARAMS[field]

    for tissue_name, tissue in cfg["tissues"].items():
        for key in ("density", "dB0", "B1+", "B1-"):
            if key in tissue:
                tissue[key] = _replace_subject(tissue[key], "subj04", subj_str)

        if tissue_name == "fat" and isinstance(tissue.get("dB0"), dict):
            tissue["dB0"]["func"] = FAT_DB0_FUNC[field]

        p = params[tissue_name]
        tissue["T1"] = p["T1"]
        tissue["T2"] = p["T2"]
        tissue["T2'"] = p["T2'"]
        tissue["ADC"] = p["ADC"]

    # Rebuild preserving key order with reslice_to inserted before tissues
    result = {}
    for key in cfg:
        if key == "reslice_to":
            continue
        if key == "tissues" and reslice is not None:
            result["reslice_to"] = reslice
        result[key] = cfg[key]

    return result


def main():
    with open("subj04.json") as f:
        template = json.load(f)

    configs = {}

    for subj in SUBJECTS:
        for field, field_str in [(3, "3T"), (7, "7T")]:
            for res_str, r in RESOLUTIONS:
                prefix = f"subj{subj:02d}-{field_str}-{res_str}"

                # 3D: no reslice_to at native 0.5mm, downsample otherwise
                reslice_3d_val = None if r == 0.5 else reslice_3d(r)
                configs[f"{prefix}.json"] = build_config(template, subj, field, reslice_3d_val)

                for orient, fn in [("tra", reslice_tra), ("cor", reslice_cor), ("sag", reslice_sag)]:
                    configs[f"{prefix}-{orient}.json"] = build_config(template, subj, field, fn(r))

    with tarfile.open("configs.tar", "w") as tar:
        for name in sorted(configs):
            data = json.dumps(configs[name], indent=2).encode("utf-8")
            info = tarfile.TarInfo(name=name)
            info.size = len(data)
            tar.addfile(info, io.BytesIO(data))

    print(f"Generated {len(configs)} configurations in configs.tar")
    # 20 subjects × 2 fields × 3 resolutions × 4 variants (1×3D + 3×2D) = 480


if __name__ == "__main__":
    main()
