use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    rc::Rc,
};

use nifti::{NiftiObject, NiftiVolume, ReaderStreamedOptions};

use crate::{
    BiftiPhantom, BiftiTissue, NiftiRef, ResliceTo,
    phantom::TissueProperty,
    resample::Resampler,
    volume::{Volume, VolumeData, map_variant},
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
        // A phantom's NIfTIs are shared between tissues (one file, one sub-volume per
        // tissue), and every tissue needs its density twice - as a map and as the
        // resampling weight. Cache the native sub-volumes so each is read exactly once.
        let mut cache = NiftiCache::default();
        let mut tissues = HashMap::new();
        for (name, tissue) in &config.tissues {
            tissues.insert(
                name.clone(),
                Tissue::load(tissue, base_dir.as_ref(), config.reslice_to, &mut cache)?,
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

/// Caches the native (unresampled) NIfTI sub-volumes a phantom refers to, keyed by file and
/// sub-volume index, so a file referenced by several tissues is only read once.
#[derive(Default)]
struct NiftiCache {
    volumes: HashMap<(PathBuf, usize), Rc<Volume>>,
}

impl NiftiCache {
    fn load(&mut self, base_dir: &Path, nifti_ref: &NiftiRef) -> Result<Rc<Volume>, crate::Error> {
        let path = base_dir.join(&nifti_ref.file_name);
        let key = (path.clone(), nifti_ref.tissue_index);
        if let Some(volume) = self.volumes.get(&key) {
            return Ok(Rc::clone(volume));
        }
        let volume = Rc::new(Volume::load_nifti_ref(&path, nifti_ref.tissue_index)?);
        self.volumes.insert(key, Rc::clone(&volume));
        Ok(volume)
    }
}

impl Volume {
    /// Load one sub-volume of a NIfTI file on its **native** grid. Resampling happens later,
    /// in [`Tissue::load`], because it needs the tissue's density map as a weight.
    #[cfg_attr(
        feature = "tracing",
        tracing::instrument(skip_all, fields(file = %path.display(), index))
    )]
    fn load_nifti_ref(path: &Path, index: usize) -> Result<Self, crate::Error> {
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

        Ok(Self {
            affine,
            shape,
            data,
        })
    }

    /// Expand a constant (1x1x1) volume onto `reslice_to`'s grid.
    fn expand_to(self, reslice_to: ResliceTo) -> Self {
        let res = reslice_to.resolution;
        Self {
            affine: reslice_to.affine,
            shape: res,
            data: self.data.expand(res[0] * res[1] * res[2]),
        }
    }
}

/// Everything needed to resample one tissue's maps: the shared geometry plus the density
/// weight that keeps intensive properties from being diluted by empty voxels.
struct TissueGrid {
    resampler: Resampler,
    reslice_to: ResliceTo,
    src_affine: [[f64; 4]; 3],
    src_shape: [usize; 3],
    /// `density + eps` on the source grid.
    weight: Vec<f64>,
    /// `sum_j w_j · weight_j` per output voxel - the shared denominator.
    weight_sum: Vec<f64>,
}

impl TissueGrid {
    fn new(density: &Volume, reslice_to: ResliceTo) -> Self {
        let resampler = Resampler::build(density.affine, density.shape, reslice_to);

        // Regularise the weight so a footprint containing no tissue at all still has a
        // well-defined mean - the unweighted mean of its in-bounds taps - instead of 0/0.
        // Scaled by the density's own magnitude, since a `density` map need not be a
        // [0, 1] float map: the spec allows any NIfTI numeric type.
        let values = density.data.to_f64_vec();
        let max = values.iter().copied().fold(0.0f64, f64::max);
        let eps = if max > 0.0 { 1e-6 * max } else { 1.0 };
        let weight: Vec<f64> = values.iter().map(|d| d + eps).collect();

        let weight_sum = resampler.weight_sum(&weight, density.shape);
        Self {
            resampler,
            reslice_to,
            src_affine: density.affine,
            src_shape: density.shape,
            weight,
            weight_sum,
        }
    }

    /// Resample one intensive property, weighted by density.
    ///
    /// The weight only makes sense if the property shares the density map's grid, which
    /// ../../NIFTI.md requires of every NIfTI in a phantom. A non-conforming file falls
    /// back to an unweighted footprint average on its own grid rather than erroring.
    fn resample(&self, native: &Volume) -> Volume {
        let data = if native.affine == self.src_affine && native.shape == self.src_shape {
            map_variant!(&native.data, |v| self.resampler.resample_weighted(
                v,
                self.src_shape,
                &self.weight,
                &self.weight_sum
            ))
        } else {
            warn_grid_mismatch();
            let resampler = Resampler::build(native.affine, native.shape, self.reslice_to);
            map_variant!(&native.data, |v| resampler.resample_plain(v, native.shape))
        };
        Volume {
            affine: self.reslice_to.affine,
            shape: self.reslice_to.resolution,
            data,
        }
    }
}

fn warn_grid_mismatch() {
    #[cfg(feature = "tracing")]
    tracing::warn!(
        "property map does not share the density map's grid (see ../../NIFTI.md); \
         resampling it unweighted, so its values may be diluted near edges"
    );
}

impl Tissue {
    #[cfg_attr(feature = "tracing", tracing::instrument(skip_all))]
    fn load(
        tissue: &BiftiTissue,
        base_dir: &Path,
        reslice_to: Option<ResliceTo>,
        cache: &mut NiftiCache,
    ) -> Result<Self, crate::Error> {
        let density_src = cache.load(base_dir, &tissue.density)?;

        let Some(reslice_to) = reslice_to else {
            // No target grid: every map keeps its own native grid, as before.
            let p = &tissue.properties;
            return Ok(Self {
                density: (*density_src).clone(),
                t1: load_native_property(cache, base_dir, &p.t1)?,
                t2: load_native_property(cache, base_dir, &p.t2)?,
                t2dash: load_native_property(cache, base_dir, &p.t2dash)?,
                adc: load_native_property(cache, base_dir, &p.adc)?,
                db0: load_native_property(cache, base_dir, &p.db0)?,
                b1_tx: load_native_channels(cache, base_dir, &p.b1_tx)?,
                b1_rx: load_native_channels(cache, base_dir, &p.b1_rx)?,
            });
        };

        let grid = TissueGrid::new(&density_src, reslice_to);

        // `density` is a volume fraction - extensive - so it is averaged *unweighted*.
        // Weighting it by itself would renormalise away exactly the partial-volume
        // information the resampling is meant to produce.
        let density = Volume {
            affine: reslice_to.affine,
            shape: reslice_to.resolution,
            data: map_variant!(&density_src.data, |v| grid
                .resampler
                .resample_plain(v, density_src.shape)),
        };

        let p = &tissue.properties;
        Ok(Self {
            density,
            t1: load_resampled_property(cache, base_dir, &p.t1, &grid)?,
            t2: load_resampled_property(cache, base_dir, &p.t2, &grid)?,
            t2dash: load_resampled_property(cache, base_dir, &p.t2dash, &grid)?,
            adc: load_resampled_property(cache, base_dir, &p.adc, &grid)?,
            db0: load_resampled_property(cache, base_dir, &p.db0, &grid)?,
            b1_tx: load_resampled_channels(cache, base_dir, &p.b1_tx, &grid)?,
            b1_rx: load_resampled_channels(cache, base_dir, &p.b1_rx, &grid)?,
        })
    }
}

/// Resolve a property on its native grid (the `reslice_to`-absent path).
fn load_native_property(
    cache: &mut NiftiCache,
    base_dir: &Path,
    property: &TissueProperty,
) -> Result<Volume, crate::Error> {
    Ok(match property {
        TissueProperty::Value(value) => Volume::single_voxel(*value),
        TissueProperty::Ref(nifti_ref) => (*cache.load(base_dir, nifti_ref)?).clone(),
        TissueProperty::Mapping(mapping) => {
            let volume = (*cache.load(base_dir, &mapping.file)?).clone();
            crate::eval::eval_mapping_func(volume, &mapping.func)?
        }
    })
}

/// Resolve every channel of a `B1+`/`B1-` array on its native grid.
fn load_native_channels(
    cache: &mut NiftiCache,
    base_dir: &Path,
    channels: &[TissueProperty],
) -> Result<Vec<Volume>, crate::Error> {
    channels
        .iter()
        .map(|ch| load_native_property(cache, base_dir, ch))
        .collect()
}

/// Resolve every channel of a `B1+`/`B1-` array onto the tissue's target grid.
fn load_resampled_channels(
    cache: &mut NiftiCache,
    base_dir: &Path,
    channels: &[TissueProperty],
    grid: &TissueGrid,
) -> Result<Vec<Volume>, crate::Error> {
    channels
        .iter()
        .map(|ch| load_resampled_property(cache, base_dir, ch, grid))
        .collect()
}

/// Resolve a property and resample it onto the tissue's target grid, weighted by density.
fn load_resampled_property(
    cache: &mut NiftiCache,
    base_dir: &Path,
    property: &TissueProperty,
    grid: &TissueGrid,
) -> Result<Volume, crate::Error> {
    Ok(match property {
        // A uniform value needs no resampling - just fill the target grid.
        TissueProperty::Value(value) => Volume::single_voxel(*value).expand_to(grid.reslice_to),
        TissueProperty::Ref(nifti_ref) => {
            let native = cache.load(base_dir, nifti_ref)?;
            grid.resample(&native)
        }
        TissueProperty::Mapping(mapping) => {
            let native = cache.load(base_dir, &mapping.file)?;
            let resampled = grid.resample(&native);
            crate::eval::eval_mapping_func(resampled, &mapping.func)?
        }
    })
}

// ===========================================================================
// Affine helpers
// ===========================================================================

/// `a ∘ b`: the affine applying `b` first, then `a`.
pub(crate) fn compose_affine(a: [[f64; 4]; 3], b: [[f64; 4]; 3]) -> [[f64; 4]; 3] {
    let mut out = [[0.0; 4]; 3];
    for (i, row) in out.iter_mut().enumerate() {
        for (j, cell) in row.iter_mut().enumerate().take(3) {
            *cell = a[i][0] * b[0][j] + a[i][1] * b[1][j] + a[i][2] * b[2][j];
        }
        row[3] = a[i][0] * b[0][3] + a[i][1] * b[1][3] + a[i][2] * b[2][3] + a[i][3];
    }
    out
}

pub(crate) fn invert_affine(a: [[f64; 4]; 3]) -> [[f64; 4]; 3] {
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
