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
- Phantom z direction should always point in $B_0$ direction

> [!note]
> In measurement and FOV, MRI sequences are assumed to be aligned to the subject.
>
> When storing phantoms, always orient them to the subject-aligned RAS+ system (origin best at center of FOV but can be arbitrary).
> Correctly stored with `sform_code == 2` and `qform` unused (`qform_code == 0`), which is the default for `nibabel` but might not for others like `simpleITK`!
