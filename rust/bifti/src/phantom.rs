use regex::Regex;
use serde::de::Error as _;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::path::PathBuf;
use std::sync::LazyLock;

pub const DEFAULT_SCHEMA: &str = "https://raw.githubusercontent.com/mrx-org/bifti-phantoms/refs/heads/main/bifti-phantom-v1.schema.json";

static SCHEMA_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(nifti|bifti)-phantom-v1(\.[^/]*)?$").unwrap());

static NIFTI_REF_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(?P<file>.+?)\[(?P<idx>\d+)\]$").unwrap());

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

#[derive(Debug, Clone, Serialize, Deserialize)]
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResliceTo {
    pub affine: [[f64; 4]; 3],
    pub resolution: [usize; 3],
}

fn default_schema() -> String {
    DEFAULT_SCHEMA.to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BiftiPhantom {
    #[serde(rename = "$schema", deserialize_with = "deserialize_schema")]
    pub schema: String,
    pub units: PhantomUnits,
    pub system: PhantomSystem,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reslice_to: Option<ResliceTo>,
    pub tissues: HashMap<String, BiftiTissue>,
}

impl Default for BiftiPhantom {
    fn default() -> Self {
        Self {
            schema: default_schema(),
            units: PhantomUnits::default(),
            system: PhantomSystem::default(),
            reslice_to: None,
            tissues: HashMap::new(),
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
