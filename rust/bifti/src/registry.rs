use crate::BiftiPhantom;
use crate::phantom::TissueProperty;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    io::Read,
    path::{Path, PathBuf},
    sync::LazyLock,
};

const REGISTRY_URL: &str =
    "https://raw.githubusercontent.com/mrx-org/bifti-phantoms/refs/heads/main/registry.json";
// A Zenodo version DOI ("10.5281/zenodo.<id>") embeds the record id
const ZENODO_FILE_URL: &str = "https://zenodo.org/api/records/{record_id}/files/{filename}/content";

static ZENODO_ID_REGEX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"zenodo\.(\d+)$").unwrap());

/// Index of public BIfTI phantoms: maps each collection name to its entry.
/// https://github.com/mrx-org/bifti-phantoms/blob/main/bifti-registry.schema.json
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Registry(HashMap<String, Collection>);

impl Registry {
    /// Download the latest registry.json from GitHub and return it parsed.
    pub fn load() -> Result<Self, crate::Error> {
        let bytes = http_get(REGISTRY_URL)?;
        Ok(serde_json::from_slice(&bytes)?)
    }

    /// Download a phantom's JSON and every NIfTI it references into
    /// `cache_dir`. Returns the path to the .json of the downloaded phantom.
    /// Re-running this does nothing as phantoms are immutable and cached.
    pub fn load_registry_phantom(
        &self,
        collection: &str,
        name: &str,
        cache_dir: &Path,
    ) -> Result<PathBuf, crate::Error> {
        let doi = &self
            .0
            .get(collection)
            .ok_or_else(|| crate::Error::CollectionLookupError(collection.to_owned()))?
            .doi;

        let dir = cache_dir.join(format!("{collection}-{}", doi.replace('/', "_")));
        std::fs::create_dir_all(&dir)?;

        let record_id = zenodo_record_id(doi)?;
        let json_path = download_json(&dir, collection, record_id, name)?;
        let phantom = BiftiPhantom::load(&json_path)?;
        for filename in collect_nifti_files(&phantom) {
            download_to(&dir, doi, &filename)?;
        }
        Ok(json_path)
    }
}

/// A registered Zenodo record and the phantom files it contains.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Collection {
    pub description: String,
    #[serde(default)]
    pub keywords: Vec<String>,
    pub authors: Vec<Author>,
    /// SPDX license identifier (e.g. "CC-BY-4.0", "CC0-1.0")
    pub license: String,
    /// Immutable Zenodo version DOI (e.g. "10.5281/zenodo.1234568")
    pub doi: String,
    /// Phantom JSON filenames inside the Zenodo record
    pub phantoms: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Author {
    /// Conventionally "Family, Given".
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub orcid: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub affiliation: Option<String>,
}

// ===========================================================================
// Internals
// ===========================================================================

fn zenodo_record_id(doi: &str) -> Result<&str, crate::Error> {
    ZENODO_ID_REGEX
        .captures(doi)
        .map(|caps| caps.get(1).unwrap().as_str())
        .ok_or_else(|| crate::Error::InvalidDoi(doi.to_owned()))
}

fn http_get(url: &str) -> Result<Vec<u8>, crate::Error> {
    let mut res = ureq::get(url).call()?;
    let mut buf = Vec::new();
    res.body_mut().as_reader().read_to_end(&mut buf)?;
    Ok(buf)
}

/// Download `filename` from the Zenodo record `doi` into `dir`. A no-op if
/// the file is already cached (the DOI guarantees identical bytes).
fn download_to(dir: &Path, doi: &str, filename: &str) -> Result<PathBuf, crate::Error> {
    let dest = dir.join(filename);
    if !dest.exists() {
        let record_id = zenodo_record_id(doi)?;
        let url = ZENODO_FILE_URL
            .replace("{record_id}", record_id)
            .replace("{filename}", filename);
        std::fs::write(&dest, http_get(&url)?)?;
    }
    Ok(dest)
}

/// Resolve a phantom JSON following the spec lookup order (REGISTRY.md):
/// try fetching `name` directly from the Zenodo record, else fall back to
/// downloading `configs.tar` and extracting `name` from it. `configs.tar` is
/// cached to `dir/configs.tar` so multiple calls only download it once.
fn download_json(
    dir: &Path,
    collection: &str,
    record_id: &str,
    name: &str,
) -> Result<PathBuf, crate::Error> {
    let dest = dir.join(name);
    if dest.exists() {
        return Ok(dest);
    }

    let url = ZENODO_FILE_URL
        .replace("{record_id}", record_id)
        .replace("{filename}", &urlencoding::encode(name));
    if let Ok(bytes) = http_get(&url) {
        std::fs::write(&dest, bytes)?;
        return Ok(dest);
    }

    let archive = dir.join("configs.tar");
    if !archive.exists() {
        let url = ZENODO_FILE_URL
            .replace("{record_id}", record_id)
            .replace("{filename}", "configs.tar");
        std::fs::write(&archive, http_get(&url)?)?;
    }

    let mut tar = tar::Archive::new(std::fs::File::open(&archive)?);
    let mut entry = tar
        .entries()?
        .filter_map(Result::ok)
        .find(|e| e.path().is_ok_and(|p| p.as_ref() == Path::new(name)))
        .ok_or_else(|| crate::Error::PhantomLookupError {
            collection: collection.to_owned(),
            phantom: name.to_owned(),
        })?;
    let mut buf = Vec::new();
    entry.read_to_end(&mut buf)?;
    std::fs::write(&dest, buf)?;
    Ok(dest)
}

fn ref_file(prop: &TissueProperty) -> Option<&Path> {
    match prop {
        TissueProperty::Ref(r) => Some(&r.file_name),
        TissueProperty::Mapping(m) => Some(&m.file.file_name),
        TissueProperty::Value(_) => None,
    }
}

/// Every distinct NIfTI filename referenced across all of a phantom's tissues.
fn collect_nifti_files(phantom: &BiftiPhantom) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut files = Vec::new();
    let mut add = |path: Option<&Path>| {
        if let Some(name) = path.and_then(Path::file_name) {
            let name = name.to_string_lossy().into_owned();
            if seen.insert(name.clone()) {
                files.push(name);
            }
        }
    };

    for tissue in phantom.tissues.values() {
        add(Some(&tissue.density.file_name));
        for prop in [
            &tissue.properties.t1,
            &tissue.properties.t2,
            &tissue.properties.t2dash,
            &tissue.properties.adc,
            &tissue.properties.db0,
        ] {
            add(ref_file(prop));
        }
        for prop in tissue.properties.b1_tx.iter().chain(&tissue.properties.b1_rx) {
            add(ref_file(prop));
        }
    }
    files
}
