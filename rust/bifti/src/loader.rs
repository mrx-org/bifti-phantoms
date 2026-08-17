use std::{collections::HashMap, path::Path};

use nifti::{NiftiObject, NiftiVolume, ReaderStreamedOptions};

use crate::{
    BiftiPhantom, BiftiTissue, NiftiRef, ResliceTo,
    phantom::TissueProperty,
    volume::{Volume, VolumeData, VolumeDataElement},
};

pub struct Phantom {
    pub config: BiftiPhantom,
    pub tissues: HashMap<String, Tissue>,
}

impl Phantom {
    #[cfg_attr(
        feature = "tracing",
        tracing::instrument(skip_all, fields(path = %path.as_ref().display()))
    )]
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self, crate::Error> {
        let path = path.as_ref().canonicalize()?;
        let config = BiftiPhantom::load(&path)?;
        let base_dir = path.parent().ok_or(crate::Error::NoParent)?;
        Self::load_from_config(config, base_dir)
    }

    #[cfg_attr(feature = "tracing", tracing::instrument(skip_all))]
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

// ===========================================================================
// Phantom loading internals
// ===========================================================================

impl VolumeData {
    fn from_nifti(slice: nifti::InMemNiftiVolume) -> Result<Self, crate::Error> {
        use nifti::NiftiType as Nt;
        Ok(match slice.data_type() {
            Nt::Uint8 => Self::Uint8(slice.into_nifti_typed_data()?),
            Nt::Uint16 => Self::Uint16(slice.into_nifti_typed_data()?),
            Nt::Uint32 => Self::Uint32(slice.into_nifti_typed_data()?),
            Nt::Uint64 => Self::Uint64(slice.into_nifti_typed_data()?),
            Nt::Int8 => Self::Int8(slice.into_nifti_typed_data()?),
            Nt::Int16 => Self::Int16(slice.into_nifti_typed_data()?),
            Nt::Int32 => Self::Int32(slice.into_nifti_typed_data()?),
            Nt::Int64 => Self::Int64(slice.into_nifti_typed_data()?),
            Nt::Float32 => Self::Float32(slice.into_nifti_typed_data()?),
            Nt::Float64 => Self::Float64(slice.into_nifti_typed_data()?),
            Nt::Complex64 => Self::Complex64(slice.into_nifti_typed_data()?),
            Nt::Complex128 => Self::Complex128(slice.into_nifti_typed_data()?),
            // Not supported: Float128, Complex256, Rgb24, Rgba32
            other => return Err(crate::Error::UnsupportedDataType(format!("{other:?}"))),
        })
    }
}

impl Volume {
    /// Resample this volume onto the grid described by `reslice_to`, using
    /// trilinear interpolation. Voxels that map outside of the source volume
    /// are set to 0.
    fn reslice(
        self,
        ResliceTo {
            affine: idx_to_world,
            resolution: res,
        }: ResliceTo,
    ) -> Result<Self, crate::Error> {
        // Treat single-voxel volumes as constant and homogeneous
        if self.shape == [1, 1, 1] {
            return Ok(Self {
                affine: idx_to_world,
                shape: res,
                data: self.data.expand(res[0] * res[1] * res[2]),
            });
        }

        let world_to_data = invert_affine(self.affine);
        use VolumeData::*;
        let resampled = match &self.data {
            Uint8(data) => Uint8(reslice(data, self.shape, res, idx_to_world, world_to_data)),
            Uint16(data) => Uint16(reslice(data, self.shape, res, idx_to_world, world_to_data)),
            Uint32(data) => Uint32(reslice(data, self.shape, res, idx_to_world, world_to_data)),
            Uint64(data) => Uint64(reslice(data, self.shape, res, idx_to_world, world_to_data)),
            Int8(data) => Int8(reslice(data, self.shape, res, idx_to_world, world_to_data)),
            Int16(data) => Int16(reslice(data, self.shape, res, idx_to_world, world_to_data)),
            Int32(data) => Int32(reslice(data, self.shape, res, idx_to_world, world_to_data)),
            Int64(data) => Int64(reslice(data, self.shape, res, idx_to_world, world_to_data)),
            Float32(data) => Float32(reslice(data, self.shape, res, idx_to_world, world_to_data)),
            Float64(data) => Float64(reslice(data, self.shape, res, idx_to_world, world_to_data)),
            Complex64(data) => {
                Complex64(reslice(data, self.shape, res, idx_to_world, world_to_data))
            }
            Complex128(data) => {
                Complex128(reslice(data, self.shape, res, idx_to_world, world_to_data))
            }
        };

        Ok(Self {
            affine: idx_to_world,
            shape: res,
            data: resampled,
        })
    }

    #[cfg_attr(
        feature = "tracing",
        tracing::instrument(skip_all, fields(file = %nifti_ref.file_name.display()))
    )]
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

        let mut volume = obj.into_volume();
        let dim = volume.dim();

        if dim.len() != 4 || index >= dim[3] as usize {
            return Err(crate::Error::IndexError {
                index,
                shape: dim.to_vec(),
            });
        }

        let shape = [dim[0] as usize, dim[1] as usize, dim[2] as usize];

        let slice = volume.nth(index).expect("index bounds checked above")?;
        let data = VolumeData::from_nifti(slice)?;

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
            TissueProperty::Value(value) => Self::single_voxel(*value),
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
    #[cfg_attr(feature = "tracing", tracing::instrument(skip_all))]
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

fn reslice<T: VolumeDataElement>(
    data: &[T],
    input_shape: [usize; 3],
    output_shape: [usize; 3],
    idx_to_world: [[f64; 4]; 3],
    world_to_data: [[f64; 4]; 3],
) -> Vec<T> {
    let mut resampled = vec![T::ZERO; output_shape[0] * output_shape[1] * output_shape[2]];

    // map the target voxel index into world-space via the
    // target's affine, then back into (continuous) source
    // volume indices via the source's inverse affine
    for ix in 0..output_shape[0] {
        for iy in 0..output_shape[1] {
            for iz in 0..output_shape[2] {
                let world = apply_affine([ix as f64, iy as f64, iz as f64], idx_to_world);
                let index = apply_affine(world, world_to_data);
                resampled[ix * output_shape[1] * output_shape[2] + iy * output_shape[2] + iz] =
                    trilinear_interp(&data, input_shape, index);
            }
        }
    }

    resampled
}

fn trilinear_interp<T: VolumeDataElement>(
    data: &[T],
    [nx, ny, nz]: [usize; 3],
    [x, y, z]: [f64; 3],
) -> T {
    let x0 = x.floor() as i64;
    let y0 = y.floor() as i64;
    let z0 = z.floor() as i64;
    let fx = x - x.floor();
    let fy = y - y.floor();
    let fz = z - z.floor();
    let (inx, iny, inz) = (nx as i64, ny as i64, nz as i64);

    let get = |xi: i64, yi: i64, zi: i64| -> T {
        if xi < 0 || xi >= inx || yi < 0 || yi >= iny || zi < 0 || zi >= inz {
            return T::ZERO;
        }
        data[xi as usize * ny * nz + yi as usize * nz + zi as usize]
    };

    let c00 = T::lerp(get(x0, y0, z0), get(x0, y0, z0 + 1), fz);
    let c01 = T::lerp(get(x0, y0 + 1, z0), get(x0, y0 + 1, z0 + 1), fz);
    let c10 = T::lerp(get(x0 + 1, y0, z0), get(x0 + 1, y0, z0 + 1), fz);
    let c11 = T::lerp(get(x0 + 1, y0 + 1, z0), get(x0 + 1, y0 + 1, z0 + 1), fz);

    let c0 = T::lerp(c00, c01, fy);
    let c1 = T::lerp(c10, c11, fy);

    T::lerp(c0, c1, fx)
}
