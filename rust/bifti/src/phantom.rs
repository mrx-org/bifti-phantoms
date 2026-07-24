use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
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
        // Find the last '[' to split filename and index
        let bracket_pos = s
            .rfind('[')
            .ok_or_else(|| format!("Invalid NiftiRef format, missing '[': {}", s))?;
        let close_bracket = s
            .rfind(']')
            .ok_or_else(|| format!("Invalid NiftiRef format, missing ']': {}", s))?;

        if close_bracket != s.len() - 1 {
            return Err(format!("Invalid NiftiRef format, ']' not at end: {}", s));
        }

        let file_name = PathBuf::from(&s[..bracket_pos]);
        let tissue_index = s[bracket_pos + 1..close_bracket]
            .parse()
            .map_err(|e| format!("Invalid tissue index: {}", e))?;

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NiftiTissueProperties {
    #[serde(rename = "T1")]
    pub t1: TissueProperty,
    #[serde(rename = "T2")]
    pub t2: TissueProperty,
    #[serde(rename = "T2'")]
    pub t2dash: TissueProperty,
    #[serde(rename = "ADC")]
    pub adc: TissueProperty,
    #[serde(rename = "dB0")]
    pub db0: TissueProperty,
    #[serde(rename = "B1+")]
    pub b1_tx: Vec<TissueProperty>,
    #[serde(rename = "B1-")]
    pub b1_rx: Vec<TissueProperty>,
}

impl Default for NiftiTissueProperties {
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BiftiTissue {
    pub density: NiftiRef,
    #[serde(default, flatten)]
    pub properties: NiftiTissueProperties,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResliceTo {
    pub affine: [[f64; 4]; 3],
    pub resolution: [usize; 3]
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct BiftiPhantom {
    pub units: PhantomUnits,
    pub system: PhantomSystem,
    pub tissues: HashMap<String, BiftiTissue>,
    pub reslice_to: Option<ResliceTo>,
    pub file_type: NiftiFileVersion,
}

/// This enum should always only contain this one field - for future file versions,
/// use a new rust file that has e.g. a NiftiPhantomV2 in here
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum NiftiFileVersion {
    #[default]
    NiftiPhantomV1,
}
