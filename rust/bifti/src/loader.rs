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

    fn reslice(self, reslice_to: ResliceTo) -> Result<Self, crate::Error> {
        todo!()
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
