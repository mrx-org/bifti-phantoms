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
> Correctly stored with `sform_code == 2` and `qform` unused (`qform_code == 0`), which is the default for `nibabel` but check for `simpleITK`!

## The NIfTI coordinate system

The "true" specification of NIfTI seems to be the C header: [nifti1_h.pdf](https://afni.nimh.nih.gov/pub/dist/doc/nifti/nifti1_h.pdf) - search for section "3D IMAGE (VOLUME) ORIENTATION AND LOCATION IN SPACE".

The specification says that NIfTIs are in **RAS+**:
> the continuous coordinates are referred to as (x,y,z). The voxel index coordinates [...] are referred to as (i,j,k) \
> [...] \
> The (x,y,z) coordinates refer to the CENTER of a voxel. **In methods 2 and 3**, the (x,y,z) axes refer to a subject-based coordinate system, with _+x = Right +y = Anterior +z = Superior_. This is a right-handed coordinate system. \
> [...] \
> The i index varies most rapidly, j index next, k index slowest.

The 3 Methods this refers to are:

1. `qform_code == 0 && sform_code == 0`: compat for ANALYZE files, only supports scaling
2. `qform_code > 0`: use a [quaternion](https://en.wikipedia.org/wiki/Quaternions_and_spatial_rotation) for additional rotation
3. `sform_code > 0`: full affine matrix specifies rotation, scale, offset (and even skewing)

**Problem** \
NIfTIs allow `qform` and `sform` to co-exist and don't specify which one to use. It states:
> In this scheme, a dataset would originally be set up so that the Method 2 coordinates represent what the scanner reported. Later, a registration to some standard space can be computed and inserted in the header. Image display software can use either transform, depending on its purposes and needs.

`nibabel` defaults to use `sform` and in normal use never sets different transforms for `sform` and `qform`, except when set manually.

### Mapping

The affine matrices map $[i,j,k] \mapsto (x, y, z)$. Even though the standard says that $(x, y, z)$ always refer to the subject-based RAS+ system, the origin of the coordinate system is specified twice, by the `qform_code` and the `sform_code`:

| code | name | description |
| --- | --- | --- |
| 0 | `UNKNOWN` | no affine provided |
| 1 | `SCANNER` | scanner coordinates |
| 2 | `ALIGNED` | arbitrary coordinate center |
| 3 | `TALAIRACH` | [Talairach coordinates - Wikipedia](https://en.wikipedia.org/wiki/Talairach_coordinates) |
| 4 | `MNI_152` | from a database which [coregistered 152 brains](https://www.bic.mni.mcgill.ca/ServicesAtlases/ICBM152NLin2009) |

### Conclusion

NIfTIs written with `nibabel` always use (unless forced) `sform_code == 2`: affine matrix with arbitrary coordinates, since the scanner system is typically not known when creating phantoms from code. Similarly, when reading files, there is usually no way to transform between scanner and subject coordinate systems, since this information is not stored in the NIfTI...

> [!warning]
> ...except if both `sform` and `qform` are used and one of them selects scanner coordinates and the other one subject coordinates. This should not be used in generated phantoms, but care must be taken when loading NIfTI files that use this property:
>
> Different viewers could decide differently which of the two systems to use - double check when debugging a wrong orientation!
>
> This information is also found in the [NIfTI FAQ](https://nifti.nimh.nih.gov/nifti-1/documentation/faq.html) - Q17 and Q19
