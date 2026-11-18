use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};
use pcbcraft_application::ports::RotationDb;
use pcbcraft_domain::cpl::{Database, CORRECTIONS_URL};

pub struct CachedRotationDb {
    pub cache_dir: Option<PathBuf>,
    pub explicit: Option<PathBuf>,
}

impl RotationDb for CachedRotationDb {
    fn load(&self, update: bool) -> Result<String> {
        if let Some(path) = &self.explicit {
            return fs::read_to_string(path)
                .with_context(|| format!("cannot read {}", path.display()));
        }
        let path = cache_dir(self.cache_dir.clone()).join("cpl_rotations_db.csv");
        if update || !path.is_file() {
            let text = download()?;
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&path, &text)?;
            return Ok(text);
        }
        fs::read_to_string(&path).with_context(|| format!("cannot read {}", path.display()))
    }
}

fn download() -> Result<String> {
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(60))
        .build();
    let response = agent
        .get(CORRECTIONS_URL)
        .set("User-Agent", "pcbcraft/0.1.0")
        .call()
        .map_err(|err| anyhow::anyhow!("downloading correction database: {err}"))?;
    let text = response
        .into_string()
        .context("correction database response")?;
    Database::parse(&text).context("downloaded file is not a correction database")?;
    Ok(text)
}

pub fn cache_dir(override_dir: Option<PathBuf>) -> PathBuf {
    if let Some(dir) = override_dir {
        return dir;
    }
    if let Some(xdg) = std::env::var_os("XDG_CACHE_HOME") {
        if !xdg.is_empty() {
            return PathBuf::from(xdg).join("pcbcraft");
        }
    }
    match std::env::var_os("HOME") {
        Some(home) if !home.is_empty() => PathBuf::from(home).join(".cache").join("pcbcraft"),
        _ => PathBuf::from(".cache").join("pcbcraft"),
    }
}

pub fn read_overlay(path: &Path) -> Result<String> {
    fs::read_to_string(path).with_context(|| format!("cannot read {}", path.display()))
}
