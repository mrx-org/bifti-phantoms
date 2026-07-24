mod loader;
mod phantom;
mod eval;

pub use loader::{Phantom, Tissue, Volume};
pub use phantom::{
    BiftiPhantom, BiftiTissue, NiftiMapping, NiftiRef, PhantomSystem, PhantomUnits, ResliceTo,
};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("file error: {0}")]
    FileError(#[from] std::io::Error),
    #[error("json error: {0}")]
    JsonError(#[from] serde_json::Error),
    #[error("path error: failed to determine the directory of the specified file")]
    NoParent,
    #[error("nifti error: {0}")]
    NiftiError(#[from] nifti::NiftiError),
    #[error("index error: tried to index {index} in 4D NIfTI, but data has shape {shape:?}")]
    IndexError{index: usize, shape: Vec<u16> },
    #[error("type error: nifti has unsupported type {0}")]
    UnsupportedDataType(String),
    #[error("mapping error: mapping functions currently only support f64 data")]
    MappingNonF64Data,
    #[error("eval error: failed to parse '{func}': {error}")]
    EvalError {func: String, error: String},
}
