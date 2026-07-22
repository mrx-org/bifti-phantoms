use num_complex::Complex64;
use std::{collections::HashMap, path::Path};
use thiserror::Error;
use toolapi::{
    AbortReason, MessageFn,
    value::{
        structured::{PhantomTissue, SegmentedPhantom, Volume},
        typed::TypedList,
    },
};

pub(crate) mod config;
mod eval;
mod nifti_loader;

use config::{NiftiPhantom, NiftiRef, NiftiTissue, TissueProperty};
use nifti_loader::Loader;

#[derive(Debug, Clone, Copy)]
pub struct Shape {
    pub res: [u32; 3],
    pub affine: [[f64; 4]; 3],
}

pub fn load(
    config_file: impl AsRef<Path>,
    shape: Shape,
    send_msg: &mut MessageFn,
) -> Result<SegmentedPhantom, PhantomError> {
    let config_path = config_file.as_ref();
    send_msg(format!("📄 Reading '{}'", config_path.display()))?;

    // Read and parse config file
    let config_str = std::fs::read_to_string(config_path)
        .map_err(|e| PhantomError::FileRead(format!("{}: {}", config_path.display(), e)))?;
    let config: NiftiPhantom = serde_json::from_str(&config_str)
        .map_err(|e| PhantomError::ConfigParse(format!("{}: {}", config_path.display(), e)))?;

    // Determine base directory from config file path
    let base_dir = config_path.parent().unwrap_or(Path::new("."));
    let mut loader = Loader::new(base_dir, shape);

    // Get the first tissue to determine grid size and spacing
    let first_tissue = config
        .tissues
        .values()
        .next()
        .ok_or_else(|| PhantomError::ConfigParse("No tissues defined in config".to_string()))?;

    // Load all tissues
    send_msg(format!(
        "📄 Config parsed, loading {} tissues",
        config.tissues.len()
    ))?;
    let tissues: Result<HashMap<String, PhantomTissue>, PhantomError> = config
        .tissues
        .iter()
        .enumerate()
        .map(|(i, (name, tissue_config))| {
            send_msg(format!(
                "📄 -------- {}/{}: {} --------",
                i + 1,
                config.tissues.len(),
                name
            ))?;
            load_tissue(tissue_config, &mut loader, send_msg)
                .map(|tissue| (name.to_owned(), tissue))
        })
        .collect();
    let tissues = tissues?;

    // Load B1 and coil_sens from first tissue (global properties)
    send_msg(format!(
        "📄 loading B1+ ({} channels)",
        first_tissue.properties.b1_tx.len()
    ))?;
    let b1_tx = load_complex_channels(&first_tissue.properties.b1_tx, &mut loader, send_msg)?;
    send_msg(format!(
        "📄 loading B1- ({} channels)",
        first_tissue.properties.b1_rx.len()
    ))?;
    let b1_rx = load_complex_channels(&first_tissue.properties.b1_rx, &mut loader, send_msg)?;

    // Create voxel shape from grid spacing
    send_msg("📄 Returning phantom".to_string())?;
    Ok(SegmentedPhantom {
        tissues,
        b1_tx,
        b1_rx,
    })
}

#[derive(Debug, Error)]
pub enum PhantomError {
    #[error("Failed to read file: {0}")]
    FileRead(String),
    #[error("Failed to parse config: {0}")]
    ConfigParse(String),
    #[error("Failed to load NIfTI: {0}")]
    NiftiLoad(String),
    #[error("Invalid tissue index {index}, max is {max}")]
    InvalidTissueIndex { index: usize, max: usize },
    #[error("Client requested aborting loading: {0}")]
    AbortLoading(#[from] AbortReason),
    #[error("Parsing mapping function `{func}` failed: {err}")]
    MappingFunction { func: String, err: String },
}

/// Load a TissueProperty into a Vec<f64> of the given size
fn load_property(
    prop: &TissueProperty,
    loader: &mut Loader,
    send_msg: &mut MessageFn,
) -> Result<Volume, PhantomError> {
    match prop {
        TissueProperty::Value(val) => Ok(Volume {
            shape: [1; 3],
            affine: [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
            ],
            data: TypedList::Float(vec![*val]),
        }),
        TissueProperty::Ref(nifti_ref) => loader.load(nifti_ref, send_msg),
        TissueProperty::Mapping(mapping) => {
            // Load the base data
            let data = loader.load(&mapping.file, send_msg)?;
            // Apply the function (simplified support - just basic operations)
            eval::eval_mapping_func(data, &mapping.func, send_msg)
        }
    }
}

/// Load a single tissue from config
fn load_tissue(
    tissue_config: &NiftiTissue,
    loader: &mut Loader,
    send_msg: &mut MessageFn,
) -> Result<PhantomTissue, PhantomError> {
    // Load density (proton density)
    let density = loader.load(&tissue_config.density, send_msg)?;

    // Load dB0 as the b0 field (off-resonance)
    let db0 = load_property(&tissue_config.properties.db0, loader, send_msg)?;

    // Load scalar tissue properties and compute averages
    let t1_map = load_property(&tissue_config.properties.t1, loader, send_msg)?;
    let t2_map = load_property(&tissue_config.properties.t2, loader, send_msg)?;
    let t2dash_map = load_property(&tissue_config.properties.t2dash, loader, send_msg)?;
    let adc_map = load_property(&tissue_config.properties.adc, loader, send_msg)?;
    let pd_map: Vec<f64> = density.data.clone().try_into().unwrap();
    let t1_map: Vec<f64> = t1_map.data.try_into().unwrap();
    let t2_map: Vec<f64> = t2_map.data.try_into().unwrap();
    let t2dash_map: Vec<f64> = t2dash_map.data.try_into().unwrap();
    let adc_map: Vec<f64> = adc_map.data.try_into().unwrap();

    // Compute PD-weighted average for tissue properties
    fn weighted_sum(pd_map: &[f64], pd_sum: f64, prop_map: &[f64]) -> Option<f64> {
        if prop_map.len() == 1 {
            // If the .json contained a constant value, we got a single-element volume
            Some(prop_map[0])
        } else if pd_sum > 0.0 {
            // TODO: we should check if the shape and affine match
            Some(pd_map.iter().zip(prop_map).map(|(p, x)| p * x).sum::<f64>() / pd_sum)
        } else {
            // Cannot calc weighted sum if sum is zero - should throw an error?
            None
        }
    }

    let pd_sum: f64 = pd_map.iter().sum();
    let t1 = weighted_sum(&pd_map, pd_sum, &t1_map).unwrap_or(f64::INFINITY);
    let t2 = weighted_sum(&pd_map, pd_sum, &t2_map).unwrap_or(f64::INFINITY);
    let t2dash = weighted_sum(&pd_map, pd_sum, &t2dash_map).unwrap_or(f64::INFINITY);
    let adc = weighted_sum(&pd_map, pd_sum, &adc_map).unwrap_or(0.0);

    send_msg(format!(
        "📄 Tissue complete (T1={:.4}, T2={:.4}, T2'={:.4}, ADC={:.4})",
        t1, t2, t2dash, adc
    ))?;
    // TODO: loader should just return volume directly
    Ok(PhantomTissue {
        density,
        db0,
        t1,
        t2,
        t2dash,
        adc,
    })
}

/// Load B1 or coil sensitivity channels (potentially complex-valued)
fn load_complex_channels(
    channels: &[TissueProperty],
    loader: &mut Loader,
    send_msg: &mut MessageFn,
) -> Result<Vec<Volume>, PhantomError> {
    channels
        .iter()
        .map(|prop| {
            // TODO: we now support all maps to have their own affine and shape:
            // get this data from the loader (only need it for caching but no longer for shape state machine)
            let mut volume = load_property(prop, loader, send_msg)?;
            // For now, treat all data as real-valued (imaginary = 0)
            // In the future, we could support complex NIfTI files
            let data: Vec<f64> = volume.data.try_into().unwrap();
            volume.data =
                TypedList::Complex(data.into_iter().map(|x| Complex64::new(x, 0.0)).collect());
            Ok(volume)
        })
        .collect()
}
