//! EasyEDA drawings and the local KiCad footprint library.

use std::path::{Path, PathBuf};

use anyhow::Result;
use pcbcraft_domain::easyeda::{LibraryFootprint, ModelPlacement};
use serde_json::Value;

#[derive(Clone)]
pub enum CadDocument {
    Found(Value),
    Missing,
}

pub trait EasyedaCad {
    fn component(&self, lcsc: &str) -> Result<CadDocument>;
    fn step(&self, uuid: &str) -> Result<Option<Vec<u8>>>;
    fn obj(&self, uuid: &str) -> Result<Option<String>>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedFootprint {
    pub footprint_path: PathBuf,
    pub step_path: Option<PathBuf>,
}

pub trait FootprintStore {
    fn save(
        &self,
        pretty_dir: &Path,
        shapes_dir: &Path,
        footprint: &LibraryFootprint,
        placement: Option<&ModelPlacement>,
        step: Option<&[u8]>,
        model_uri: &str,
    ) -> Result<SavedFootprint>;

    fn register(&self, table: &Path, uri: &str) -> Result<()>;
}
