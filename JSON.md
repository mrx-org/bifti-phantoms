# JSON Phantom File

The **phantom JSON file** defines a numerical MRI simulation phantom: a set of
*tissues*, each carrying physical MR properties (T1, T2, …). A property is
either a single number (spatially uniform) or a reference to a sub-volume of a
**NIfTI** file stored next to the JSON (see [NIFTI.md](NIFTI.md)). Files are
validated against
[`bifti-phantom-v1.schema.json`](bifti-phantom-v1.schema.json)
(JSON Schema draft 2020-12). For the overall format and folder layout, see
[SPEC.md](SPEC.md).

## Top-level structure

```jsonc
{
  "$schema": "…/bifti-phantom-v1.schema.json",  // older nifti-... is supported as well
  "units":   { … },     // fixed, documentation only
  "system":  { … },     // global MR system parameters
  "patient": { … },     // optional patient position in the scanner
  "reslice_to": { … },  // optional resampling grid
  "tissues": { … }      // the tissues
}
```

| Field        | Required | Type   | Purpose                                          |
|--------------|----------|--------|--------------------------------------------------|
| `$schema`    | yes      | string | Identifies the format and version.               |
| `units`      | yes      | object | Fixed unit table (documentation, see below).     |
| `system`     | yes      | object | Global parameters shared by all tissues.         |
| `patient`    | no       | object | How the subject lies in the scanner.             |
| `reslice_to` | no       | object | Optional target grid to resample all NIfTIs onto.|
| `tissues`    | yes      | object | One or more named tissues.                       |

Unknown top-level keys are permitted: the format is additively extensible, so a
reader must ignore what it does not recognise (and should warn about it). See
[SPEC.md](SPEC.md) for the versioning rule.

### `$schema`

Doubles as the **format discriminator / version tag** and as the pointer
editors use to locate the schema. Any URI whose path ends in
`bifti-phantom-v1` (or the older `nifti-phantom-v1`) is accepted. Recommended:
https://raw.githubusercontent.com/mrx-org/bifti-phantoms/refs/heads/main/bifti-phantom-v1.schema.json

### `units`

A **fixed** object that must appear verbatim. Units are not configurable in this
version so that parsers never have to convert; this field exists so a file is
self-documenting. More units might be added in future revisions.

| Quantity | Unit            |
|----------|-----------------|
| `gyro`   | `MHz/T`         |
| `B0`     | `T`             |
| `T1`     | `s`             |
| `T2`     | `s`             |
| `T2'`    | `s`             |
| `ADC`    | `10^-3 mm^2/s`  |
| `dB0`    | `Hz`            |
| `B1+`    | `rel`           |
| `B1-`    | `rel`           |

### `system`

Global scalars for the (virtual) MR system.

| Field  | Required | Type   | Meaning                                            |
|--------|----------|--------|----------------------------------------------------|
| `B0`   | yes      | number | Main field strength the data was captured for [T]. |
| `gyro` | yes      | number | Gyromagnetic ratio [MHz/T] (`42.5764` for water).  |

### `patient` (optional)

How the subject is positioned in the scanner. Phantom data is always stored
subject-aligned in RAS+, while MRI sequences are written in scanner coordinates —
`patient` is what lets a consumer convert between the two.

| Field      | Required | Type   | Meaning                                              |
|------------|----------|--------|------------------------------------------------------|
| `position` | yes      | string | DICOM-style patient position code (see table below). |

```json
"patient": { "position": "HFS" }
```

`position` is one of eight codes, spelled in uppercase:

| Code   | Meaning                              |
|--------|--------------------------------------|
| `FFS`  | feet first, supine                   |
| `FFP`  | feet first, prone                    |
| `FFDR` | feet first, decubitus right          |
| `FFDL` | feet first, decubitus left           |
| `HFS`  | head first, supine                   |
| `HFP`  | head first, prone                    |
| `HFDR` | head first, decubitus right          |
| `HFDL` | head first, decubitus left           |

Each code defines a rotation from phantom (RAS+) to scanner coordinates; the
scanner coordinate system and the matrix belonging to each code are defined in
[NIFTI.md](NIFTI.md#patient-position).

**If `patient` is omitted, the position is `FFS`** — the identity, i.e. no
transform at all. A phantom that says nothing about positioning is therefore
never silently rotated.

`patient` is metadata only. It never changes the stored voxel data, the NIfTI
affines, or `reslice_to`; those always stay subject-aligned.

### `reslice_to` (optional)

If omitted, every NIfTI is loaded as-is. If given, **all** NIfTIs are resampled
onto the specified grid, interpreted exactly as in the NIfTI standard. This
changes only how the data is sampled, never the orientation of the phantom.

| Field        | Type               | Meaning                                                                       |
|--------------|--------------------|-------------------------------------------------------------------------------|
| `affine`     | `number[3][4]`     | Upper 3 rows of the 4×4 voxel-to-world affine (implicit 4th row `[0,0,0,1]`). |
| `resolution` | `integer[3]` (≥ 1) | Target matrix size (voxel counts) along the 3 spatial axes.                   |

Both fields are required when `reslice_to` is present.

### `tissues`

An object mapping a tissue **name** to its definition. At least one tissue is
required. Keys are arbitrary identifiers (`gm`, `wm`, `csf`, `fat`, …); they
carry no meaning beyond labelling.

## Tissue

Each tissue has a spatial distribution plus a fixed set of physical properties.
Only the properties below are allowed; any omitted property takes its default.

| Property  | Required | Value                  | Unit       | Default    |
|-----------|----------|------------------------|------------|------------|
| `density` | yes      | NIfTI reference        | _fraction_ | —          |
| `T1`      | no       | scalar-or-map          | s          | `infinity` |
| `T2`      | no       | scalar-or-map          | s          | `infinity` |
| `T2'`     | no       | scalar-or-map          | s          | `infinity` |
| `ADC`     | no       | scalar-or-map          | 10⁻³ mm²/s | `0`        |
| `dB0`     | no       | scalar-or-map          | Hz         | `0`        |
| `B1+`     | no       | array of scalar-or-map | _rel_      | `[1]`      |
| `B1-`     | no       | array of scalar-or-map | _rel_      | `[1]`      |

- `density` is the tissue's volume fraction and must be spatially resolved (a
  plain NIfTI reference).
- `B1+` / `B1-` are **arrays**, one entry per transmit / receive channel.

### Scalar-or-map

A single property value is one of:

1. **a number** — spatially uniform across the whole phantom like `"T1": 1.5`
2. **a NIfTI reference** — a spatially varying map
3. **a transformed reference** — a NIfTI map with a per-voxel expression applied.

### NIfTI reference

A string naming a NIfTI file (stored next to the JSON) with a **mandatory**
sub-volume index — the zero-based position along the file's 4th dimension — in
square brackets:

```json
{
  "density": "subj42.nii.gz[0]",
  "dB0": "subj42_dB0.nii[3]"
}
```

### Transformed reference

A NIfTI reference whose voxel values are remapped:

```json
"dB0": { "file": "subj42_dB0.nii.gz[0]", "func": "x - 420" }
```

`func` is an expression evaluated per voxel. Allowed tokens:

- **numbers** — integer, decimal, leading-dot, or scientific notation
  (`420`, `-1.5`, `.5`, `1e-3`);
- **operators** `+ - * /` and **parentheses** `( )`;
- **variables** `x` (the voxel value) and the per-volume statistics
  `x_min`, `x_max`, `x_std`, `x_mean`.

---
