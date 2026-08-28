use regex::Regex;
use serde::de::Error as _;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

pub const DEFAULT_SCHEMA: &str = "https://raw.githubusercontent.com/mrx-org/bifti-phantoms/refs/heads/main/bifti-phantom-v1.schema.json";

// Matches both the `$schema` URL (using hyphens, e.g. "nifti-phantom-v1.schema.json")
// and the legacy plain `file_type` value (using underscores, e.g. "nifti_phantom_v1").
static SCHEMA_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(nifti|bifti)[-_]phantom[-_]v1(\.[^/]*)?$").unwrap());

static NIFTI_REF_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(?P<file>.+?)\[(?P<idx>\d+)\]$").unwrap());

// The format is additively extensible (../../../SPEC.md), so unknown fields are
// ignored rather than rejected - but the schema no longer catches typos either,
// which leaves the reader as the only place that can point one out.
macro_rules! warn_unknown {
    ($($arg:tt)*) => {{
        let msg = format!($($arg)*);
        #[cfg(feature = "tracing")]
        tracing::warn!("{}", msg);
        #[cfg(not(feature = "tracing"))]
        eprintln!("bifti: {msg}");
    }};
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PhantomUnits {
    pub gyro: String,
    #[serde(rename = "B0")]
    pub b0: String,
    #[serde(rename = "T1")]
    pub t1: String,
    #[serde(rename = "T2")]
    pub t2: String,
    #[serde(rename = "T2'")]
    pub t2dash: String,
    #[serde(rename = "ADC")]
    pub adc: String,
    #[serde(rename = "dB0")]
    pub db0: String,
    #[serde(rename = "B1+")]
    pub b1_tx: String,
    #[serde(rename = "B1-")]
    pub b1_rx: String,
}

impl Default for PhantomUnits {
    fn default() -> Self {
        Self {
            gyro: "MHz/T".to_string(),
            b0: "T".to_string(),
            t1: "s".to_string(),
            t2: "s".to_string(),
            t2dash: "s".to_string(),
            adc: "10^-3 mm^2/s".to_string(),
            db0: "Hz".to_string(),
            b1_tx: "rel".to_string(),
            b1_rx: "rel".to_string(),
        }
    }
}

// Only the default units are supported for now, mirroring the Python
// implementation's `assert default.to_dict() == config`.
impl<'de> Deserialize<'de> for PhantomUnits {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Raw {
            gyro: String,
            #[serde(rename = "B0")]
            b0: String,
            #[serde(rename = "T1")]
            t1: String,
            #[serde(rename = "T2")]
            t2: String,
            #[serde(rename = "T2'")]
            t2dash: String,
            #[serde(rename = "ADC")]
            adc: String,
            #[serde(rename = "dB0")]
            db0: String,
            #[serde(rename = "B1+")]
            b1_tx: String,
            #[serde(rename = "B1-")]
            b1_rx: String,
        }

        let raw = Raw::deserialize(deserializer)?;
        let units = PhantomUnits {
            gyro: raw.gyro,
            b0: raw.b0,
            t1: raw.t1,
            t2: raw.t2,
            t2dash: raw.t2dash,
            adc: raw.adc,
            db0: raw.db0,
            b1_tx: raw.b1_tx,
            b1_rx: raw.b1_rx,
        };

        if units != PhantomUnits::default() {
            return Err(D::Error::custom(format!(
                "Only default units are supported for now, got {units:?}"
            )));
        }
        Ok(units)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(default)]
pub struct PhantomSystem {
    pub gyro: f64,
    #[serde(rename = "B0")]
    pub b0: f64,
}

impl Default for PhantomSystem {
    fn default() -> Self {
        Self {
            gyro: 42.5764,
            b0: 3.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct NiftiRef {
    pub file_name: PathBuf,
    pub tissue_index: usize,
}

impl From<NiftiRef> for String {
    fn from(r: NiftiRef) -> String {
        r.to_string()
    }
}

impl fmt::Display for NiftiRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}[{}]", self.file_name.display(), self.tissue_index)
    }
}

impl TryFrom<String> for NiftiRef {
    type Error = String;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        let caps = NIFTI_REF_REGEX
            .captures(&s)
            .ok_or_else(|| format!("Invalid file_ref: {s}"))?;

        let file_name = PathBuf::from(&caps["file"]);
        let tissue_index = caps["idx"].parse().expect("regex should only allow ints");

        Ok(Self {
            file_name,
            tissue_index,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NiftiMapping {
    pub file: NiftiRef,
    pub func: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum TissueProperty {
    Value(f64),
    Ref(NiftiRef),
    Mapping(NiftiMapping),
}

impl From<f64> for TissueProperty {
    fn from(value: f64) -> Self {
        TissueProperty::Value(value)
    }
}

impl From<NiftiRef> for TissueProperty {
    fn from(value: NiftiRef) -> Self {
        TissueProperty::Ref(value)
    }
}

impl From<NiftiMapping> for TissueProperty {
    fn from(value: NiftiMapping) -> Self {
        TissueProperty::Mapping(value)
    }
}

fn is_default_relaxation(prop: &TissueProperty) -> bool {
    matches!(prop, TissueProperty::Value(v) if *v == f64::INFINITY)
}

fn is_default_zero(prop: &TissueProperty) -> bool {
    matches!(prop, TissueProperty::Value(v) if *v == 0.0)
}

// serde's skip_serializing_if requires a fn(&Vec<T>) -> bool, not fn(&[T]) -> bool
#[allow(clippy::ptr_arg)]
fn is_default_b1_channels(channels: &Vec<TissueProperty>) -> bool {
    matches!(channels.as_slice(), [TissueProperty::Value(v)] if *v == 1.0)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TissueProperties {
    #[serde(rename = "T1", skip_serializing_if = "is_default_relaxation")]
    pub t1: TissueProperty,
    #[serde(rename = "T2", skip_serializing_if = "is_default_relaxation")]
    pub t2: TissueProperty,
    #[serde(rename = "T2'", skip_serializing_if = "is_default_relaxation")]
    pub t2dash: TissueProperty,
    #[serde(rename = "ADC", skip_serializing_if = "is_default_zero")]
    pub adc: TissueProperty,
    #[serde(rename = "dB0", skip_serializing_if = "is_default_zero")]
    pub db0: TissueProperty,
    #[serde(rename = "B1+", skip_serializing_if = "is_default_b1_channels")]
    pub b1_tx: Vec<TissueProperty>,
    #[serde(rename = "B1-", skip_serializing_if = "is_default_b1_channels")]
    pub b1_rx: Vec<TissueProperty>,
}

impl Default for TissueProperties {
    fn default() -> Self {
        Self {
            t1: TissueProperty::Value(f64::INFINITY),
            t2: TissueProperty::Value(f64::INFINITY),
            t2dash: TissueProperty::Value(f64::INFINITY),
            adc: TissueProperty::Value(0.0),
            db0: TissueProperty::Value(0.0),
            b1_tx: vec![TissueProperty::Value(1.0)],
            b1_rx: vec![TissueProperty::Value(1.0)],
        }
    }
}

// density has no sensible default (the schema requires it), so it can't live
// in a struct with a single container-level #[serde(default)] alongside the
// properties below. Keep it as a sibling field and flatten the defaultable
// properties into their own struct instead.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BiftiTissue {
    pub density: NiftiRef,
    #[serde(flatten)]
    pub properties: TissueProperties,
    /// Fields this version of the crate doesn't know, kept so that saving a
    /// phantom round-trips them instead of quietly dropping them.
    #[serde(flatten)]
    pub unknown: serde_json::Map<String, serde_json::Value>,
}

/// A DICOM-style patient position code.
///
/// Defines the rotation from phantom RAS+ into scanner coordinates. The scanner
/// frame is right-handed with Z along B0 pointing *out of* the bore and Y
/// pointing up, which makes [`PatientPosition::FeetFirstSupine`] the identity -
/// and therefore the default, so a phantom that states no position is never
/// transformed. See ../../../NIFTI.md#patient-position.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum PatientPosition {
    #[default]
    #[serde(rename = "FFS")]
    FeetFirstSupine,
    #[serde(rename = "FFP")]
    FeetFirstProne,
    #[serde(rename = "FFDR")]
    FeetFirstDecubitusRight,
    #[serde(rename = "FFDL")]
    FeetFirstDecubitusLeft,
    #[serde(rename = "HFS")]
    HeadFirstSupine,
    #[serde(rename = "HFP")]
    HeadFirstProne,
    #[serde(rename = "HFDR")]
    HeadFirstDecubitusRight,
    #[serde(rename = "HFDL")]
    HeadFirstDecubitusLeft,
}

impl PatientPosition {
    /// The 3x3 rotation from phantom RAS+ into scanner coordinates, i.e.
    /// `v_scanner = to_scanner() * v_ras`. Always a proper rotation (det = +1).
    pub fn to_scanner(self) -> [[f64; 3]; 3] {
        match self {
            Self::FeetFirstSupine => [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            Self::FeetFirstProne => [[-1.0, 0.0, 0.0], [0.0, -1.0, 0.0], [0.0, 0.0, 1.0]],
            Self::FeetFirstDecubitusRight => [[0.0, 1.0, 0.0], [-1.0, 0.0, 0.0], [0.0, 0.0, 1.0]],
            Self::FeetFirstDecubitusLeft => [[0.0, -1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]],
            Self::HeadFirstSupine => [[-1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, -1.0]],
            Self::HeadFirstProne => [[1.0, 0.0, 0.0], [0.0, -1.0, 0.0], [0.0, 0.0, -1.0]],
            Self::HeadFirstDecubitusRight => [[0.0, -1.0, 0.0], [-1.0, 0.0, 0.0], [0.0, 0.0, -1.0]],
            Self::HeadFirstDecubitusLeft => [[0.0, 1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, -1.0]],
        }
    }

    /// The same rotation as a 4x4 affine, for composing with a voxel-to-RAS one.
    pub fn to_scanner_affine(self) -> [[f64; 4]; 4] {
        let r = self.to_scanner();
        [
            [r[0][0], r[0][1], r[0][2], 0.0],
            [r[1][0], r[1][1], r[1][2], 0.0],
            [r[2][0], r[2][1], r[2][2], 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ]
    }
}

/// How the subject lies in the scanner (../../../JSON.md -> `patient`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Patient {
    pub position: PatientPosition,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct ResliceTo {
    pub affine: [[f64; 4]; 3],
    pub resolution: [usize; 3],
}

fn default_schema() -> String {
    DEFAULT_SCHEMA.to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BiftiPhantom {
    #[serde(
        rename = "$schema",
        alias = "file_type",
        deserialize_with = "deserialize_schema"
    )]
    pub schema: String,
    pub units: PhantomUnits,
    pub system: PhantomSystem,
    /// `None` means `FFS`: an unpositioned phantom is never transformed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub patient: Option<Patient>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reslice_to: Option<ResliceTo>,
    pub tissues: HashMap<String, BiftiTissue>,
    /// Fields this version of the crate doesn't know, kept so that saving a
    /// phantom round-trips them instead of quietly dropping them.
    #[serde(flatten)]
    pub unknown: serde_json::Map<String, serde_json::Value>,
}

impl BiftiPhantom {
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self, crate::Error> {
        let phantom: Self =
            serde_json::from_reader(std::io::BufReader::new(std::fs::File::open(path)?))?;
        phantom.warn_unknown_fields();
        Ok(phantom)
    }

    /// Warn about every field that this version of the crate doesn't recognize.
    ///
    /// Called by [`BiftiPhantom::load`]; call it yourself if you deserialized a
    /// phantom some other way.
    pub fn warn_unknown_fields(&self) {
        for key in self.unknown.keys() {
            warn_unknown!("Ignoring unknown field {key:?} in the phantom");
        }
        for (name, tissue) in &self.tissues {
            for key in tissue.unknown.keys() {
                warn_unknown!("Ignoring unknown field {key:?} in tissue {name:?}");
            }
        }
    }

    /// The 3x3 phantom-RAS+ -> scanner rotation of this phantom.
    ///
    /// The identity when no `patient` is given (../../../NIFTI.md#patient-position).
    pub fn to_scanner_matrix(&self) -> [[f64; 3]; 3] {
        self.patient.unwrap_or_default().position.to_scanner()
    }

    pub fn save<P: AsRef<Path>>(&self, path: P) -> Result<(), crate::Error> {
        // If there is a directory specified, try to create it first
        if let Some(dir) = path.as_ref().parent() {
            std::fs::create_dir_all(dir)?;
        }
        serde_json::to_writer(std::io::BufWriter::new(std::fs::File::create(path)?), self)?;
        Ok(())
    }

    /// Returns a list of all nifti files referenced by this phantom
    pub fn referenced_nifti_files(&self) -> Vec<PathBuf> {
        let mut files = HashSet::new();

        fn extract<'a>(files: &mut HashSet<&'a PathBuf>, prop: &'a TissueProperty) {
            match prop {
                TissueProperty::Value(_) => false,
                TissueProperty::Ref(nifti_ref) => files.insert(&nifti_ref.file_name),
                TissueProperty::Mapping(nifti_mapping) => {
                    files.insert(&nifti_mapping.file.file_name)
                }
            };
        }

        for tissue in self.tissues.values() {
            files.insert(&tissue.density.file_name);

            extract(&mut files, &tissue.properties.t1);
            extract(&mut files, &tissue.properties.t2);
            extract(&mut files, &tissue.properties.t2dash);
            extract(&mut files, &tissue.properties.adc);
            extract(&mut files, &tissue.properties.db0);
            for channel in &tissue.properties.b1_tx {
                extract(&mut files, channel);
            }
            for channel in &tissue.properties.b1_rx {
                extract(&mut files, channel);
            }
        }

        files.into_iter().cloned().collect()
    }
}

impl Default for BiftiPhantom {
    fn default() -> Self {
        Self {
            schema: default_schema(),
            units: PhantomUnits::default(),
            system: PhantomSystem::default(),
            patient: None,
            reslice_to: None,
            tissues: HashMap::new(),
            unknown: serde_json::Map::new(),
        }
    }
}

fn deserialize_schema<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let schema = String::deserialize(deserializer)?;
    if !SCHEMA_REGEX.is_match(&schema) {
        return Err(D::Error::custom(format!("Unsupported $schema: {schema:?}")));
    }
    Ok(schema)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL_POSITIONS: [PatientPosition; 8] = [
        PatientPosition::FeetFirstSupine,
        PatientPosition::FeetFirstProne,
        PatientPosition::FeetFirstDecubitusRight,
        PatientPosition::FeetFirstDecubitusLeft,
        PatientPosition::HeadFirstSupine,
        PatientPosition::HeadFirstProne,
        PatientPosition::HeadFirstDecubitusRight,
        PatientPosition::HeadFirstDecubitusLeft,
    ];

    fn det(m: [[f64; 3]; 3]) -> f64 {
        m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
            - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
            + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
    }

    #[test]
    fn ffs_is_the_identity_and_the_default() {
        let identity = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
        assert_eq!(PatientPosition::default(), PatientPosition::FeetFirstSupine);
        assert_eq!(PatientPosition::FeetFirstSupine.to_scanner(), identity);
        // A phantom without a `patient` must not be transformed at all.
        assert_eq!(BiftiPhantom::default().to_scanner_matrix(), identity);
    }

    #[test]
    fn hfs_turns_the_subject_about_the_vertical_axis() {
        // Head first is feet first rotated 180 deg about Y, so R and S flip.
        assert_eq!(
            PatientPosition::HeadFirstSupine.to_scanner(),
            [[-1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, -1.0]]
        );
    }

    #[test]
    fn every_position_is_a_proper_rotation() {
        for pos in ALL_POSITIONS {
            let m = pos.to_scanner();
            assert!((det(m) - 1.0).abs() < 1e-12, "{pos:?} has det {}", det(m));

            // Orthonormal: M * M^T == I
            for i in 0..3 {
                for j in 0..3 {
                    let dot: f64 = (0..3).map(|k| m[i][k] * m[j][k]).sum();
                    let expected = if i == j { 1.0 } else { 0.0 };
                    assert_eq!(dot, expected, "{pos:?} rows {i},{j}");
                }
            }
        }
    }

    #[test]
    fn superior_axis_says_which_end_goes_into_the_bore() {
        for pos in ALL_POSITIONS {
            // Scanner Z points out of the bore, so head-first maps S onto -Z.
            let head_first = matches!(
                pos,
                PatientPosition::HeadFirstSupine
                    | PatientPosition::HeadFirstProne
                    | PatientPosition::HeadFirstDecubitusRight
                    | PatientPosition::HeadFirstDecubitusLeft
            );
            let s_z = pos.to_scanner()[2][2];
            assert_eq!(s_z, if head_first { -1.0 } else { 1.0 }, "{pos:?}");
        }
    }

    #[test]
    fn positions_round_trip_through_their_dicom_codes() {
        for (pos, code) in ALL_POSITIONS
            .iter()
            .zip(["FFS", "FFP", "FFDR", "FFDL", "HFS", "HFP", "HFDR", "HFDL"])
        {
            let json = serde_json::to_string(pos).unwrap();
            assert_eq!(json, format!("\"{code}\""));
            assert_eq!(
                serde_json::from_str::<PatientPosition>(&json).unwrap(),
                *pos
            );
        }
        // Codes are case-sensitive, and DICOM's non-MR codes are not supported.
        assert!(serde_json::from_str::<PatientPosition>("\"hfs\"").is_err());
        assert!(serde_json::from_str::<PatientPosition>("\"SITTING\"").is_err());
    }

    fn phantom_json(extra: &str) -> String {
        format!(
            r#"{{
                "$schema": "bifti-phantom-v1.schema.json",
                "units": {{
                    "gyro": "MHz/T", "B0": "T", "T1": "s", "T2": "s", "T2'": "s",
                    "ADC": "10^-3 mm^2/s", "dB0": "Hz", "B1+": "rel", "B1-": "rel"
                }},
                "system": {{ "gyro": 42.5764, "B0": 3.0 }},
                {extra}
                "tissues": {{ "gm": {{ "density": "x.nii.gz[0]", "T1": 1.5 }} }}
            }}"#
        )
    }

    #[test]
    fn patient_is_optional_and_round_trips() {
        let without: BiftiPhantom = serde_json::from_str(&phantom_json("")).unwrap();
        assert_eq!(without.patient, None);
        assert!(!serde_json::to_string(&without).unwrap().contains("patient"));

        let with: BiftiPhantom =
            serde_json::from_str(&phantom_json(r#""patient": { "position": "HFDR" },"#)).unwrap();
        assert_eq!(
            with.patient,
            Some(Patient {
                position: PatientPosition::HeadFirstDecubitusRight
            })
        );
        assert_eq!(
            with.to_scanner_matrix(),
            PatientPosition::HeadFirstDecubitusRight.to_scanner()
        );

        let reparsed: BiftiPhantom =
            serde_json::from_str(&serde_json::to_string(&with).unwrap()).unwrap();
        assert_eq!(reparsed.patient, with.patient);
    }

    #[test]
    fn unknown_fields_are_kept_rather_than_rejected() {
        let json = phantom_json(r#""from_the_future": { "a": 1 },"#);
        let phantom: BiftiPhantom = serde_json::from_str(&json).unwrap();
        assert!(phantom.unknown.contains_key("from_the_future"));
        phantom.warn_unknown_fields();

        // Saving must not silently drop what we didn't understand.
        let round_tripped: BiftiPhantom =
            serde_json::from_str(&serde_json::to_string(&phantom).unwrap()).unwrap();
        assert_eq!(round_tripped.unknown, phantom.unknown);

        // Same at tissue level, where a typo is the likelier cause.
        let json = json.replace(r#""T1": 1.5"#, r#""T1": 1.5, "T22": 0.1"#);
        let phantom: BiftiPhantom = serde_json::from_str(&json).unwrap();
        assert!(phantom.tissues["gm"].unknown.contains_key("T22"));
    }
}
