use glam::DMat4;
use nifti::{InMemNiftiObject, InMemNiftiVolume, NiftiObject, NiftiType, NiftiVolume};
use std::{collections::HashMap, path::Path};
use toolapi::{
    MessageFn,
    value::{structured::Volume, typed::TypedList},
};

use super::{NiftiRef, PhantomError, Shape};

/// This loader caches the loaded nifti files. It also ensures that all of them
/// have the same resolution (which is currently expected by toolapi / sims)
pub struct Loader {
    base_dir: std::path::PathBuf,
    shape: Shape,
    cache: HashMap<std::path::PathBuf, Vec<Volume>>,
}

impl Loader {
    pub fn new(base_dir: impl AsRef<Path>, shape: Shape) -> Self {
        Self {
            base_dir: base_dir.as_ref().to_path_buf(),
            shape,
            cache: HashMap::new(),
        }
    }

    pub fn load(
        &mut self,
        nifti_ref: &NiftiRef,
        send_msg: &mut MessageFn,
    ) -> Result<Volume, PhantomError> {
        // Load whole file (with all tissues) into cache if not yet there
        let full_path = self.base_dir.join(&nifti_ref.file_name);
        if !self.cache.contains_key(&full_path) {
            let nifti = Nifti::load(full_path.clone(), self.shape, send_msg)?;

            self.cache.insert(full_path.clone(), nifti.volumes);
            send_msg(format!(
                "⚙ Caching file, {} files now in cache",
                self.cache.len()
            ))?;
        } else {
            send_msg(format!("⚙ Using cached file {}", full_path.display()))?;
        }

        // Now we have ensured that the resampled nifti is there - return tissue
        let data = self.cache.get(&full_path).unwrap();
        data.get(nifti_ref.tissue_index)
            .cloned()
            .ok_or(PhantomError::InvalidTissueIndex {
                index: nifti_ref.tissue_index,
                max: data.len() - 1,
            })
    }
}

struct Nifti {
    volumes: Vec<Volume>,
}

impl Nifti {
    fn load(
        full_path: std::path::PathBuf,
        shape: Shape,
        send_msg: &mut MessageFn,
    ) -> Result<Self, PhantomError> {
        // get the correct mip level
        let mip = shape.res.iter().max().unwrap().next_power_of_two().min(512);
        // Fix file name: deviating from the standard, we use zstd to compress and have mips
        let full_path = full_path.with_file_name(format!(
            "{}-mip{mip}.nii.zst",
            full_path.file_prefix().unwrap().display()
        ));

        send_msg(format!("⚙ Loading file {}", full_path.display()))?;

        let start = std::time::Instant::now();
        let reader = zstd::Decoder::new(
            std::fs::File::open(&full_path)
                .map_err(|e| PhantomError::NiftiLoad(format!("{}: {}", full_path.display(), e)))?,
        )
        .map_err(|e| PhantomError::NiftiLoad(e.to_string()))?;
        let nifti = InMemNiftiObject::from_reader(reader)
            .map_err(|e| PhantomError::NiftiLoad(e.to_string()))?;
        send_msg(format!(
            "⚙ Loading took {} s",
            start.elapsed().as_secs_f32()
        ))?;

        // Extract header BEFORE into_volume() consumes the object
        let src_affine = [
            nifti.header().srow_x.map(|x| x as f64),
            nifti.header().srow_y.map(|x| x as f64),
            nifti.header().srow_z.map(|x| x as f64),
        ];

        let volume = nifti.into_volume();
        let dim = volume.dim();

        // Validate 4D data
        if dim.len() != 4 {
            return Err(PhantomError::NiftiLoad(format!(
                "Expected 4D NIfTI, got {}D",
                dim.len()
            )));
        }

        let src_shape = [dim[0] as usize, dim[1] as usize, dim[2] as usize];
        let num_tissues = dim[3] as usize;

        send_msg(format!(
            "⚙ Loaded data: shape={:?}, num_tissues={}",
            src_shape, num_tissues,
        ))?;

        let dst_shape = [
            shape.res[0] as usize,
            shape.res[1] as usize,
            shape.res[2] as usize,
        ];

        // Get raw data converted to f64
        let time = std::time::Instant::now();
        let raw_data = volume_to_f64(volume, &full_path)?;
        send_msg(format!(
            "⚠ Volume to f64 took {:.2} s",
            time.elapsed().as_secs_f32()
        ))?;

        let volume_size = src_shape[0] * src_shape[1] * src_shape[2];

        // Precompute the mapping matrix: maps output voxel indices to source
        // voxel indices. This is src_affine_inv * dst_affine.
        let src_mat = affine_3x4_to_dmat4(src_affine);
        let src_inv = src_mat.inverse();
        let dst_mat = affine_3x4_to_dmat4(shape.affine);
        let mapping = src_inv * dst_mat;

        // Split 4D into Vec of 3D volumes and resample each.
        send_msg(format!(
            "⚙ Resampling from {:?} to {:?}",
            src_shape, dst_shape
        ))?;
        let volumes = (0..num_tissues)
            .map(|i| {
                send_msg(format!("⚙ Processing volume {}", i))?;

                // Extract the tissue volume from the flat 4D array
                // NIfTI stores data in column-major (Fortran) order: x changes fastest
                // For 4D: index = x + y*dim_x + z*dim_x*dim_y + t*dim_x*dim_y*dim_z
                let tissue_start = i * volume_size;
                let tissue_end = tissue_start + volume_size;
                let tissue_data = &raw_data[tissue_start..tissue_end];

                let resampled =
                    resample_affine_nearest(tissue_data, src_shape, dst_shape, &mapping);

                Ok(Volume {
                    shape: dst_shape.map(|x| x as u64),
                    affine: shape.affine,
                    data: TypedList::Float(resampled),
                })
            })
            .collect::<Result<Vec<Volume>, PhantomError>>()?;

        send_msg(format!(
            "⚙ Resampled {} volumes to shape={:?}",
            volumes.len(),
            shape.res,
        ))?;

        Ok(Nifti { volumes })
    }
}

// ============================================================================
// Affine helpers and resampling
// ============================================================================

/// Convert a 3x4 affine (as stored in NIfTI headers and our Shape) to a glam
/// DMat4. The implicit 4th row is [0, 0, 0, 1].
fn affine_3x4_to_dmat4(affine: [[f64; 4]; 3]) -> DMat4 {
    // DMat4::from_cols takes column vectors. Our affine rows are:
    //   row 0: [r00 r01 r02 t0]   (world_x = r00*i + r01*j + r02*k + t0)
    //   row 1: [r10 r11 r12 t1]
    //   row 2: [r20 r21 r22 t2]
    // implicit: [ 0   0   0  1]
    DMat4::from_cols_array(&[
        // column 0
        affine[0][0],
        affine[1][0],
        affine[2][0],
        0.0,
        // column 1
        affine[0][1],
        affine[1][1],
        affine[2][1],
        0.0,
        // column 2
        affine[0][2],
        affine[1][2],
        affine[2][2],
        0.0,
        // column 3 (translation)
        affine[0][3],
        affine[1][3],
        affine[2][3],
        1.0,
    ])
}

/// Resample source data (Fortran-order) into the target grid (C-order) using
/// nearest-neighbor interpolation. The `mapping` matrix transforms output voxel
/// indices (i, j, k) directly into source voxel indices. Voxels that map
/// outside the source volume are set to 0.
fn resample_affine_nearest(
    src_data: &[f64],
    src_shape: [usize; 3],
    dst_shape: [usize; 3],
    mapping: &DMat4,
) -> Vec<f64> {
    let total = dst_shape[0] * dst_shape[1] * dst_shape[2];
    let mut result = Vec::with_capacity(total);

    let [sx, sy, _sz] = src_shape;

    // Decompose the mapping matrix into column vectors for efficient computation.
    // For output voxel (x, y, z):
    //   src = mapping * [x, y, z, 1]^T
    //   src_i = col0[i]*x + col1[i]*y + col2[i]*z + col3[i]
    let col0 = mapping.col(0);
    let col1 = mapping.col(1);
    let col2 = mapping.col(2);
    let col3 = mapping.col(3); // translation

    // Write output in C order: x outermost, z innermost
    for x in 0..dst_shape[0] {
        let xf = x as f64;
        // Precompute the x contribution
        let bx = col0.x * xf + col3.x;
        let by = col0.y * xf + col3.y;
        let bz = col0.z * xf + col3.z;

        for y in 0..dst_shape[1] {
            let yf = y as f64;
            // Precompute x + y contribution
            let cx = bx + col1.x * yf;
            let cy = by + col1.y * yf;
            let cz = bz + col1.z * yf;

            for z in 0..dst_shape[2] {
                let zf = z as f64;
                // Full source coordinates
                let si = cx + col2.x * zf;
                let sj = cy + col2.y * zf;
                let sk = cz + col2.z * zf;

                // Round to nearest integer
                let si = si.round() as i64;
                let sj = sj.round() as i64;
                let sk = sk.round() as i64;

                // Bounds check
                if si >= 0
                    && sj >= 0
                    && sk >= 0
                    && (si as usize) < src_shape[0]
                    && (sj as usize) < src_shape[1]
                    && (sk as usize) < src_shape[2]
                {
                    // Source is Fortran order: x changes fastest
                    let idx = si as usize + sj as usize * sx + sk as usize * sx * sy;
                    result.push(src_data[idx]);
                } else {
                    result.push(0.0);
                }
            }
        }
    }

    result
}

/// Load NIfTI volume data into Vec<f64>, handling different underlying data types
fn volume_to_f64(
    volume: InMemNiftiVolume,
    path: &std::path::Path,
) -> Result<Vec<f64>, PhantomError> {
    let data_type = volume.data_type();
    let map_err = |e| PhantomError::NiftiLoad(format!("{}: {}", path.display(), e));

    match data_type {
        NiftiType::Uint8 => {
            let data: Vec<u8> = volume.into_nifti_typed_data().map_err(map_err)?;
            Ok(data.into_iter().map(|v| v as f64 / 255.0).collect())
        }
        NiftiType::Float32 => {
            let data: Vec<f32> = volume.into_nifti_typed_data().map_err(map_err)?;
            Ok(data.into_iter().map(|v| v as f64).collect())
        }
        NiftiType::Float64 => volume.into_nifti_typed_data().map_err(map_err),
        _ => Err(PhantomError::NiftiLoad(format!(
            "{}: unsupported data type {:?}",
            path.display(),
            data_type
        ))),
    }
}
