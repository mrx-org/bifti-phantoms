# NIfTI Files

Per-voxel phantom data is stored in NIfTI files that are referenced from the
[phantom JSON file](JSON.md). This page covers the NIfTI format requirements and
the coordinate-system conventions those files must follow.

## Data format

Per-voxel tissue properties are stored in `.nii` files following the
[NIfTI v1.1](https://nifti.nimh.nih.gov/nifti-1/) specification, optionally
gzip-compressed (`.nii.gz`).

- Each file contains a single property for all tissues
- Data must be 4-dimensional (use singleton dimensions for non-3D data)
  - Dimensions 1-3: spatial (size 1 if unused)
  - Dimension 4: tissue index
- All NIfTI files must share the same resolution and orientation
- Spatial data should follow the RAS+ convention (index 0: R, 1: A, 2: S, growing towards positive) to ensure correct orientation for tools ignoring the affine matrix
- The affine matrix must transform data into RAS+ using mm as units (as per NIfTI spec)

## Coordinate system

- BIfTI phantoms always use RAS+ in a subject-aligned coordinate system
- NIfTIs can store two orientations at once and do not specify which one to use
- MITK uses a LPS+ coordinate system and negates the xy affine entries on loading
- The scanner says data is in the `SCANNER` coordinate system, but this changes with sequence settings.
- The phantom z direction is parallel to $B_0$, but its sign depends on how the
  subject lies in the scanner - see [Patient position](#patient-position)

> [!note]
> In measurement and FOV, MRI sequences are assumed to be aligned to the subject.
>
> When storing phantoms, always orient them to the subject-aligned RAS+ system (origin best at center of FOV but can be arbitrary).
> Correctly stored with `sform_code == 2` and `qform` unused (`qform_code == 0`), which is the default for `nibabel` but might not for others like `simpleITK`!

## Patient position

Phantom data is subject-aligned, sequences are written in scanner coordinates.
The optional [`patient.position`](JSON.md#patient-optional) field is what relates
the two.

### Scanner coordinate system

Right-handed, fixed to the magnet:

- **Z** - along $B_0$, pointing **out of the bore** (opposite to the direction
  the table travels when moving in)
- **Y** - vertical, pointing up (against gravity)
- **X** = $Y \times Z$, completing the right-handed frame

Equivalently, and this is the anchor the table below is built on: **for a
feet-first supine subject, the scanner X/Y/Z coincide with the subject's R/A/S**.
`FFS` is therefore the identity.

### Transform

`position` defines a signed permutation matrix $P$ with

$$v_\text{scanner} = P \cdot v_\text{RAS}$$

$P$ is a proper rotation ($\det P = +1$) for every code. The columns of $P$ are
the images of the R, A and S axes:

| Code   | R &rarr; | A &rarr; | S &rarr; | $P$                                    |
|--------|----------|----------|----------|----------------------------------------|
| `FFS`  | $+X$     | $+Y$     | $+Z$     | identity                               |
| `FFP`  | $-X$     | $-Y$     | $+Z$     | `diag(-1, -1,  1)`                     |
| `FFDR` | $-Y$     | $+X$     | $+Z$     | `[[0,  1, 0], [-1, 0,  0], [0, 0,  1]]` |
| `FFDL` | $+Y$     | $-X$     | $+Z$     | `[[0, -1, 0], [ 1, 0,  0], [0, 0,  1]]` |
| `HFS`  | $-X$     | $+Y$     | $-Z$     | `diag(-1,  1, -1)`                     |
| `HFP`  | $+X$     | $-Y$     | $-Z$     | `diag( 1, -1, -1)`                     |
| `HFDR` | $-Y$     | $-X$     | $-Z$     | `[[0, -1, 0], [-1, 0,  0], [0, 0, -1]]` |
| `HFDL` | $+Y$     | $+X$     | $-Z$     | `[[0,  1, 0], [ 1, 0,  0], [0, 0, -1]]` |

"Decubitus right / left" means the subject lies on that side (that side down),
as in DICOM. DICOM's left-first and sitting codes are not supported; they are
reserved for a future revision.

Two properties are worth checking the table against:

- every head-first code has $S \rightarrow -Z$ (superior points into the bore),
  every feet-first code has $S \rightarrow +Z$
- `HFS` is `FFS` rotated by 180&deg; about the vertical axis, `diag(-1, 1, -1)` -
  exactly what turning a supine subject end-for-end on the table does

### Using it

- voxel &rarr; scanner affine: $A_\text{scanner} = P_4 \cdot A_\text{RAS}$,
  where $P_4$ is $P$ extended to 4x4 with a `[0, 0, 0, 1]` row and column
- a direction given in scanner coordinates (a gradient axis, a slice normal)
  maps into phantom RAS+ via $P^T$ - $P$ is orthogonal, so $P^{-1} = P^T$

> [!note]
> **An omitted `patient` means `FFS`, i.e. $P$ = identity.** A phantom that does
> not state a position is never transformed, so adding the field to the spec
> changes the behaviour of no existing phantom.
