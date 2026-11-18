//! `pcbcraft.yaml` next to the project.

use serde::Deserialize;
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Config {
    pub skipped: Vec<String>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ConfigError {
    #[error("pcbcraft.yaml: {0}")]
    Yaml(String),
    #[error("pcbcraft.yaml: skipped entry must not be empty")]
    EmptySkip,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileConfig {
    #[serde(default)]
    tests: TestsConfig,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct TestsConfig {
    #[serde(default)]
    skipped: Vec<String>,
}

pub fn parse_config(text: &str) -> Result<Config, ConfigError> {
    if text.trim().is_empty() {
        return Ok(Config::default());
    }
    let file: FileConfig =
        serde_yaml::from_str(text).map_err(|err| ConfigError::Yaml(err.to_string()))?;
    if file
        .tests
        .skipped
        .iter()
        .any(|entry| entry.trim().is_empty())
    {
        return Err(ConfigError::EmptySkip);
    }
    Ok(Config {
        skipped: file.tests.skipped,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_skipped_ids() {
        let config = parse_config("tests:\n  skipped:\n    - VoltageRating.C21\n").unwrap();
        assert_eq!(config.skipped, vec!["VoltageRating.C21"]);
    }

    #[test]
    fn an_unknown_key_is_rejected() {
        let err = parse_config("tests:\n  skip:\n    - VoltageRating.C21\n").unwrap_err();
        assert!(err.to_string().contains("pcbcraft.yaml"));
    }

    #[test]
    fn a_blank_skip_is_rejected() {
        assert_eq!(
            parse_config("tests:\n  skipped: ['']\n").unwrap_err(),
            ConfigError::EmptySkip
        );
    }
}
