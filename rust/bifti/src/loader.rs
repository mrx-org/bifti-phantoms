use std::{collections::HashMap, path::Path};

use nifti::{NiftiObject, NiftiVolume, ReaderStreamedOptions};
use num_complex::Complex;

use crate::{BiftiPhantom, BiftiTissue, NiftiRef, ResliceTo, phantom::TissueProperty};

pub struct Phantom {
    pub config: BiftiPhantom,
    pub tissues: HashMap<String, Tissue>,
}

impl Phantom {
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self, crate::Error> {
        let path = path.as_ref().canonicalize()?;
        let config = BiftiPhantom::load(&path)?;
        let base_dir = path.parent().ok_or(crate::Error::NoParent)?;
        Self::load_from_config(config, base_dir)
    }

    pub fn load_from_config<P: AsRef<Path>>(
        config: BiftiPhantom,
        base_dir: P,
    ) -> Result<Self, crate::Error> {
        let mut tissues = HashMap::new();
        for (name, tissue) in &config.tissues {
            tissues.insert(
                name.clone(),
                Tissue::load(tissue, base_dir.as_ref(), config.reslice_to)?,
            );
        }
        Ok(Self { config, tissues })
    }
}

pub struct Tissue {
    pub density: Volume,
    pub t1: Volume,
    pub t2: Volume,
    pub t2dash: Volume,
    pub adc: Volume,
    pub db0: Volume,
    pub b1_tx: Vec<Volume>,
    pub b1_rx: Vec<Volume>,
}

pub struct Volume {
    pub affine: [[f64; 4]; 3],
    pub shape: [usize; 3],
    pub data: VolumeData,
}

pub enum VolumeData {
    Float32(Vec<f32>),
    Float64(Vec<f64>),
    Complex32(Complex<f32>),
    Complex64(Complex<f64>),
}

// ===========================================================================
// Phantom loading internals
// ===========================================================================

impl Volume {
    fn from_f64(value: f64) -> Self {
        Self {
            affine: [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
            ],
            shape: [1, 1, 1],
            data: VolumeData::Float64(vec![value]),
        }
    }

    /// Resample this volume onto the grid described by `reslice_to`, using
    /// trilinear interpolation. Voxels that map outside of the source volume
    /// are set to 0.
    fn reslice(self, reslice_to: ResliceTo) -> Result<Self, crate::Error> {
        let input: Vec<f64> = match &self.data {
            VolumeData::Float32(data) => data.iter().map(|&x| x as f64).collect(),
            VolumeData::Float64(data) => data.clone(),
            VolumeData::Complex32(_) | VolumeData::Complex64(_) => {
                return Err(crate::Error::UnsupportedDataType(
                    "reslicing complex-valued data is not supported".to_string(),
                ));
            }
        };

        let inv_input_affine = invert_affine(self.affine);
        let res = reslice_to.resolution;
        let mut resampled = vec![0.0f64; res[0] * res[1] * res[2]];

        for ix in 0..res[0] {
            for iy in 0..res[1] {
                for iz in 0..res[2] {
                    // map the target voxel index into world-space via the
                    // target's affine, then back into (continuous) source
                    // volume indices via the source's inverse affine
                    let world = apply_affine([ix as f64, iy as f64, iz as f64], reslice_to.affine);
                    let index = apply_affine(world, inv_input_affine);
                    resampled[ix * res[1] * res[2] + iy * res[2] + iz] =
                        trilinear_interp(&input, self.shape, index);
                }
            }
        }

        Ok(Self {
            affine: reslice_to.affine,
            shape: res,
            data: VolumeData::Float64(resampled),
        })
    }

    fn load_nifti_ref(
        base_dir: &Path,
        nifti_ref: &NiftiRef,
        reslice_to: Option<ResliceTo>,
    ) -> Result<Self, crate::Error> {
        let path = base_dir.join(&nifti_ref.file_name);
        let index = nifti_ref.tissue_index;

        let obj = ReaderStreamedOptions::new().read_file(path)?;

        let header = obj.header();
        let affine = [
            header.srow_x.map(|v| v as f64),
            header.srow_y.map(|v| v as f64),
            header.srow_z.map(|v| v as f64),
        ];

        let volume = obj.into_volume();
        let dim = volume.dim();

        if dim.len() != 4 || index >= dim[3] as usize {
            return Err(crate::Error::IndexError {
                index,
                shape: dim.to_vec(),
            });
        }

        let shape = [dim[0] as usize, dim[1] as usize, dim[2] as usize];

        let slice = volume
            .skip(index)
            .next()
            .expect("index bounds checked above")?;

        let data = match slice.data_type() {
            nifti::NiftiType::Float32 => VolumeData::Float32(slice.into_nifti_typed_data()?),
            nifti::NiftiType::Float64 => VolumeData::Float64(slice.into_nifti_typed_data()?),
            // nifti::NiftiType::Complex64 => VolumeData::Complex32(slice.into_nifti_typed_data()?),
            // nifti::NiftiType::Complex128 => VolumeData::Complex64(slice.into_nifti_typed_data()?),
            other => return Err(crate::Error::UnsupportedDataType(format!("{other:?}"))),
        };

        let volume = Self {
            affine,
            shape,
            data,
        };

        match reslice_to {
            Some(reslice_to) => volume.reslice(reslice_to),
            None => Ok(volume),
        }
    }

    fn load_tissue_property(
        base_dir: &Path,
        tissue_property: &TissueProperty,
        reslice_to: Option<ResliceTo>,
    ) -> Result<Self, crate::Error> {
        let volume = match tissue_property {
            TissueProperty::Value(value) => Self::from_f64(*value),
            TissueProperty::Ref(nifti_ref) => {
                Self::load_nifti_ref(base_dir, nifti_ref, reslice_to)?
            }
            TissueProperty::Mapping(nifti_mapping) => {
                let volume = Self::load_nifti_ref(base_dir, &nifti_mapping.file, reslice_to)?;
                crate::eval::eval_mapping_func(volume, &nifti_mapping.func)?
            }
        };

        match reslice_to {
            Some(reslice_to) => volume.reslice(reslice_to),
            None => Ok(volume),
        }
    }
}

impl Tissue {
    fn load(
        tissue: &BiftiTissue,
        base_dir: &Path,
        reslice_to: Option<ResliceTo>,
    ) -> Result<Self, crate::Error> {
        let density = Volume::load_nifti_ref(base_dir, &tissue.density, reslice_to)?;

        Ok(Self {
            density,
            t1: Volume::load_tissue_property(base_dir, &tissue.properties.t1, reslice_to)?,
            t2: Volume::load_tissue_property(base_dir, &tissue.properties.t2, reslice_to)?,
            t2dash: Volume::load_tissue_property(base_dir, &tissue.properties.t2dash, reslice_to)?,
            adc: Volume::load_tissue_property(base_dir, &tissue.properties.adc, reslice_to)?,
            db0: Volume::load_tissue_property(base_dir, &tissue.properties.db0, reslice_to)?,
            // TODO: b1 tx rx don't load complex values (see old mod.rs)
            b1_tx: tissue
                .properties
                .b1_tx
                .iter()
                .map(|ch| Volume::load_tissue_property(base_dir, ch, reslice_to))
                .collect::<Result<Vec<_>, _>>()?,
            b1_rx: tissue
                .properties
                .b1_rx
                .iter()
                .map(|ch| Volume::load_tissue_property(base_dir, ch, reslice_to))
                .collect::<Result<Vec<_>, _>>()?,
        })
    }
}

// ===========================================================================
// Affine helpers
// ===========================================================================

fn apply_affine(vec: [f64; 3], affine: [[f64; 4]; 3]) -> [f64; 3] {
    [
        affine[0][0] * vec[0] + affine[0][1] * vec[1] + affine[0][2] * vec[2] + affine[0][3],
        affine[1][0] * vec[0] + affine[1][1] * vec[1] + affine[1][2] * vec[2] + affine[1][3],
        affine[2][0] * vec[0] + affine[2][1] * vec[1] + affine[2][2] * vec[2] + affine[2][3],
    ]
}

fn invert_affine(a: [[f64; 4]; 3]) -> [[f64; 4]; 3] {
    let inv_det = 1.0
        / (a[0][0] * (a[1][1] * a[2][2] - a[1][2] * a[2][1])
            - a[0][1] * (a[1][0] * a[2][2] - a[1][2] * a[2][0])
            + a[0][2] * (a[1][0] * a[2][1] - a[1][1] * a[2][0]));

    let r = [
        [
            (a[1][1] * a[2][2] - a[1][2] * a[2][1]) * inv_det,
            (a[0][2] * a[2][1] - a[0][1] * a[2][2]) * inv_det,
            (a[0][1] * a[1][2] - a[0][2] * a[1][1]) * inv_det,
        ],
        [
            (a[1][2] * a[2][0] - a[1][0] * a[2][2]) * inv_det,
            (a[0][0] * a[2][2] - a[0][2] * a[2][0]) * inv_det,
            (a[0][2] * a[1][0] - a[0][0] * a[1][2]) * inv_det,
        ],
        [
            (a[1][0] * a[2][1] - a[1][1] * a[2][0]) * inv_det,
            (a[0][1] * a[2][0] - a[0][0] * a[2][1]) * inv_det,
            (a[0][0] * a[1][1] - a[0][1] * a[1][0]) * inv_det,
        ],
    ];

    // inverse 3x4 matrix of the input - offset is mapped to the new system
    [
        [
            r[0][0],
            r[0][1],
            r[0][2],
            -(r[0][0] * a[0][3] + r[0][1] * a[1][3] + r[0][2] * a[2][3]),
        ],
        [
            r[1][0],
            r[1][1],
            r[1][2],
            -(r[1][0] * a[0][3] + r[1][1] * a[1][3] + r[1][2] * a[2][3]),
        ],
        [
            r[2][0],
            r[2][1],
            r[2][2],
            -(r[2][0] * a[0][3] + r[2][1] * a[1][3] + r[2][2] * a[2][3]),
        ],
    ]
}

fn trilinear_interp(data: &[f64], [nx, ny, nz]: [usize; 3], [x, y, z]: [f64; 3]) -> f64 {
    let x0 = x.floor() as i64;
    let y0 = y.floor() as i64;
    let z0 = z.floor() as i64;
    let fx = x - x.floor();
    let fy = y - y.floor();
    let fz = z - z.floor();
    let (inx, iny, inz) = (nx as i64, ny as i64, nz as i64);

    let get = |xi: i64, yi: i64, zi: i64| -> f64 {
        if xi < 0 || xi >= inx || yi < 0 || yi >= iny || zi < 0 || zi >= inz {
            return 0.0;
        }
        data[xi as usize * ny * nz + yi as usize * nz + zi as usize]
    };

    let c00 = get(x0, y0, z0) * (1.0 - fz) + get(x0, y0, z0 + 1) * fz;
    let c01 = get(x0, y0 + 1, z0) * (1.0 - fz) + get(x0, y0 + 1, z0 + 1) * fz;
    let c10 = get(x0 + 1, y0, z0) * (1.0 - fz) + get(x0 + 1, y0, z0 + 1) * fz;
    let c11 = get(x0 + 1, y0 + 1, z0) * (1.0 - fz) + get(x0 + 1, y0 + 1, z0 + 1) * fz;

    let c0 = c00 * (1.0 - fy) + c01 * fy;
    let c1 = c10 * (1.0 - fy) + c11 * fy;

    c0 * (1.0 - fx) + c1 * fx
}
