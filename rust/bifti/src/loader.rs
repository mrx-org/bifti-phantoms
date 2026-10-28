use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    rc::Rc,
};

use nifti::{NiftiObject, NiftiVolume, ReaderStreamedOptions};

use crate::{
    BiftiPhantom, BiftiTissue, NiftiRef, ResliceTo,
    nifti_write::write_stack,
    phantom::TissueProperty,
    resample::Resampler,
    volume::{Volume, VolumeData, map_variant},
};

pub struct Phantom {
    pub config: BiftiPhantom,
    pub tissues: HashMap<String, Tissue>,
}

/// Options for [`Phantom::load_with`] and [`Phantom::load_from_config_with`]. Override
/// single fields with `LoadOptions { max_substeps: 16, ..Default::default() }`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LoadOptions {
    /// Caps the quadrature samples per axis of each output voxel when an oblique
    /// `reslice_to` cannot use the exact separable average. Raise it when the warning says
    /// the result is under-sampled; axis-aligned grids ignore it. Default: 8.
    pub max_substeps: usize,
}

impl Default for LoadOptions {
    fn default() -> Self {
        Self { max_substeps: 8 }
    }
}

impl Phantom {
    /// [`Phantom::load_with`] with the default [`LoadOptions`].
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self, crate::Error> {
        Self::load_with(path, LoadOptions::default())
    }

    /// Load a phantom JSON and every NIfTI it references, relative to the JSON's folder.
    #[cfg_attr(
        feature = "tracing",
        tracing::instrument(skip_all, fields(path = %path.as_ref().display()))
    )]
    pub fn load_with<P: AsRef<Path>>(path: P, options: LoadOptions) -> Result<Self, crate::Error> {
        let path = path.as_ref().canonicalize()?;
        let config = BiftiPhantom::load(&path)?;
        let base_dir = path.parent().ok_or(crate::Error::NoParent)?;
        Self::load_from_config_with(config, base_dir, options)
    }

    /// [`Phantom::load_from_config_with`] with the default [`LoadOptions`].
    pub fn load_from_config<P: AsRef<Path>>(
        config: BiftiPhantom,
        base_dir: P,
    ) -> Result<Self, crate::Error> {
        Self::load_from_config_with(config, base_dir, LoadOptions::default())
    }

    /// Load the NIfTIs of an already parsed (possibly edited) config from `base_dir`.
    #[cfg_attr(feature = "tracing", tracing::instrument(skip_all))]
    pub fn load_from_config_with<P: AsRef<Path>>(
        config: BiftiPhantom,
        base_dir: P,
        options: LoadOptions,
    ) -> Result<Self, crate::Error> {
        // A phantom's NIfTIs are shared between tissues (one file, one sub-volume per
        // tissue), and every tissue needs its density twice - as a map and as the
        // resampling weight. Cache the native sub-volumes so each is read exactly once.
        let mut cache = NiftiCache::default();
        let mut tissues = HashMap::new();
        for (name, tissue) in &config.tissues {
            tissues.insert(
                name.clone(),
                Tissue::load(
                    tissue,
                    base_dir.as_ref(),
                    config.reslice_to,
                    options,
                    &mut cache,
                )?,
            );
        }
        Ok(Self { config, tissues })
    }

    /// Save the phantom as a self-contained directory: the NIfTIs plus the phantom JSON
    /// `out_dir/json_name`, whose path is returned.
    ///
    /// This writes what [`Phantom::load`] produced, so loading with a `reslice_to` and then
    /// saving is how a phantom is resliced on disk. The saved phantom has no `reslice_to`
    /// and **no `func` mappings**: every mapping was already applied to the resampled data,
    /// and its reference becomes a plain one.
    ///
    /// Layout: one gzip-compressed NIfTI per property (`density.nii.gz`, `T1.nii.gz`,
    /// `B1plus_0.nii.gz`, ...) with the tissues stacked along the 4th dimension in order of
    /// their sorted names. Properties that are constants in the phantom stay constants in
    /// the JSON. Output is deterministic: the same phantom always gives the same bytes.
    ///
    /// # Size
    ///
    /// The result can be considerably **larger than the input**. Tissues that referenced
    /// the same map (with or without different `func`s) each get their own copy of it,
    /// unless the copies are bit-identical, in which case they share one volume. When
    /// reslicing, intensive properties are weighted by each tissue's own density, so
    /// copies of one source map usually differ between tissues. Mixed element types within
    /// one property are promoted to `Float64` (`Complex128` if any is complex).
    pub fn save<P: AsRef<Path>>(
        &self,
        out_dir: P,
        json_name: &str,
    ) -> Result<PathBuf, crate::Error> {
        let out_dir = out_dir.as_ref();
        std::fs::create_dir_all(out_dir)?;

        let mut config = self.config.clone();
        config.reslice_to = None;

        let mut names: Vec<String> = config.tissues.keys().cloned().collect();
        names.sort();
        if let Some(missing) = names.iter().find(|n| !self.tissues.contains_key(*n)) {
            return Err(crate::Error::SaveError(format!(
                "tissue {missing:?} is in the config but was not loaded"
            )));
        }

        let channels = |select: fn(&BiftiTissue) -> usize| {
            names
                .iter()
                .map(|n| select(&config.tissues[n]))
                .max()
                .unwrap_or(0)
        };
        let n_tx = channels(|t| t.properties.b1_tx.len());
        let n_rx = channels(|t| t.properties.b1_rx.len());
        let fields = [
            Field::Density,
            Field::T1,
            Field::T2,
            Field::T2dash,
            Field::Adc,
            Field::Db0,
        ]
        .into_iter()
        .chain((0..n_tx).map(Field::B1Tx))
        .chain((0..n_rx).map(Field::B1Rx));

        for field in fields {
            // Constants stay in the JSON; only maps get a NIfTI.
            let members: Vec<&String> = names
                .iter()
                .filter(|n| field.is_map(&config.tissues[*n]))
                .collect();
            if members.is_empty() {
                continue;
            }
            let volumes = members
                .iter()
                .map(|n| {
                    field.volume(&self.tissues[*n]).ok_or_else(|| {
                        crate::Error::SaveError(format!(
                            "tissue {n:?} has no loaded {}",
                            field.stem()
                        ))
                    })
                })
                .collect::<Result<Vec<_>, _>>()?;

            let file_name = PathBuf::from(format!("{}.nii.gz", field.stem()));
            let slots = write_stack(&out_dir.join(&file_name), &volumes)?;
            for (name, slot) in members.into_iter().zip(slots) {
                let tissue = config
                    .tissues
                    .get_mut(name)
                    .expect("name comes from the config");
                field.set_ref(
                    tissue,
                    NiftiRef {
                        file_name: file_name.clone(),
                        tissue_index: slot,
                    },
                );
            }
        }

        let json_path = out_dir.join(json_name);
        config.save(&json_path)?;
        Ok(json_path)
    }
}

/// One per-tissue map of a phantom, as [`Phantom::save`] groups them into files.
#[derive(Clone, Copy)]
enum Field {
    Density,
    T1,
    T2,
    T2dash,
    Adc,
    Db0,
    B1Tx(usize),
    B1Rx(usize),
}

impl Field {
    /// File name stem; the filename-safe spelling of the JSON key.
    fn stem(self) -> String {
        match self {
            Self::Density => "density".into(),
            Self::T1 => "T1".into(),
            Self::T2 => "T2".into(),
            Self::T2dash => "T2dash".into(),
            Self::Adc => "ADC".into(),
            Self::Db0 => "dB0".into(),
            Self::B1Tx(k) => format!("B1plus_{k}"),
            Self::B1Rx(k) => format!("B1minus_{k}"),
        }
    }

    fn property(self, tissue: &BiftiTissue) -> Option<&TissueProperty> {
        let p = &tissue.properties;
        match self {
            Self::Density => None,
            Self::T1 => Some(&p.t1),
            Self::T2 => Some(&p.t2),
            Self::T2dash => Some(&p.t2dash),
            Self::Adc => Some(&p.adc),
            Self::Db0 => Some(&p.db0),
            Self::B1Tx(k) => p.b1_tx.get(k),
            Self::B1Rx(k) => p.b1_rx.get(k),
        }
    }

    /// Whether the tissue's property is backed by a NIfTI (rather than a constant or absent).
    fn is_map(self, tissue: &BiftiTissue) -> bool {
        match self {
            Self::Density => true,
            _ => matches!(
                self.property(tissue),
                Some(TissueProperty::Ref(_) | TissueProperty::Mapping(_))
            ),
        }
    }

    fn volume(self, tissue: &Tissue) -> Option<&Volume> {
        match self {
            Self::Density => Some(&tissue.density),
            Self::T1 => Some(&tissue.t1),
            Self::T2 => Some(&tissue.t2),
            Self::T2dash => Some(&tissue.t2dash),
            Self::Adc => Some(&tissue.adc),
            Self::Db0 => Some(&tissue.db0),
            Self::B1Tx(k) => tissue.b1_tx.get(k),
            Self::B1Rx(k) => tissue.b1_rx.get(k),
        }
    }

    /// Point the tissue's property at `nifti_ref`, replacing any `func` mapping.
    fn set_ref(self, tissue: &mut BiftiTissue, nifti_ref: NiftiRef) {
        let p = &mut tissue.properties;
        let slot = match self {
            Self::Density => {
                tissue.density = nifti_ref;
                return;
            }
            Self::T1 => &mut p.t1,
            Self::T2 => &mut p.t2,
            Self::T2dash => &mut p.t2dash,
            Self::Adc => &mut p.adc,
            Self::Db0 => &mut p.db0,
            Self::B1Tx(k) => &mut p.b1_tx[k],
            Self::B1Rx(k) => &mut p.b1_rx[k],
        };
        *slot = TissueProperty::Ref(nifti_ref);
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
    max_substeps: usize,
}

impl TissueGrid {
    fn new(density: &Volume, reslice_to: ResliceTo, max_substeps: usize) -> Self {
        let resampler = Resampler::build(density.affine, density.shape, reslice_to, max_substeps);

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
            max_substeps,
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
            let resampler = Resampler::build(
                native.affine,
                native.shape,
                self.reslice_to,
                self.max_substeps,
            );
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
        options: LoadOptions,
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

        let grid = TissueGrid::new(&density_src, reslice_to, options.max_substeps);

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
