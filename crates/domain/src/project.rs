//! Paths that name a KiCad project. Existence checks stay in the adapter.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Project {
    pub directory: PathBuf,
    pub pro: Option<PathBuf>,
    pub schematic: Option<PathBuf>,
    pub board: Option<PathBuf>,
}

impl Project {
    pub fn cache_dir(&self) -> PathBuf {
        self.directory.join(".pcbcraft")
    }

    pub fn net_cache(&self) -> PathBuf {
        self.cache_dir().join("net.sqlite")
    }

    pub fn component_db(&self) -> PathBuf {
        self.cache_dir().join("components.sqlite3")
    }

    pub fn stem(&self) -> String {
        let source = self
            .pro
            .as_deref()
            .or(self.board.as_deref())
            .or(self.schematic.as_deref());
        source
            .and_then(Path::file_stem)
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_else(|| "plytka".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_files_sit_next_to_the_project() {
        let project = Project {
            directory: PathBuf::from("/boards/radio"),
            pro: Some(PathBuf::from("/boards/radio/radio.kicad_pro")),
            schematic: None,
            board: None,
        };
        assert_eq!(project.stem(), "radio");
        assert_eq!(
            project.net_cache(),
            PathBuf::from("/boards/radio/.pcbcraft/net.sqlite")
        );
        assert_eq!(
            project.component_db(),
            PathBuf::from("/boards/radio/.pcbcraft/components.sqlite3")
        );
    }
}
