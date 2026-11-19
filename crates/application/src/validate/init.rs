//! Default test files for `pcbcraft tests init defaults`.

use std::path::{Component, Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde_json::json;

const STARTER: &[(&str, &str)] = &[
    ("erc-test.json", "ERCValidator"),
    ("drc-test.json", "DRCValidator"),
    ("availability-test.json", "PartAvailability"),
];

pub fn scaffold_body(validator: &str, project: &str) -> Option<String> {
    let spec = match validator {
        "PartAvailability" => json!({ "project": project, "defaultMinimum": 10, "part": {} }),
        "ResistorPower" | "VoltageRating" | "Divider" => json!({ "project": project, "rails": {} }),
        "JLCPCBDFM" => json!({
            "project": project,
            "maskColor": "green",
            "rails": [],
            "allowOverhang": []
        }),
        "ERCValidator" | "DRCValidator" | "Attributes" | "CatalogValue" | "TimeConstant"
        | "ParallelPassives" | "SeriesPassives" => json!({ "project": project }),
        _ => return None,
    };
    let value = json!({
        "type": validator,
        "spec": spec
    });
    let mut text = serde_json::to_string_pretty(&value).unwrap_or_else(|_| "{}".into());
    text.push('\n');
    Some(text)
}

/// Create `dir` and write the starter ERC, DRC, and availability tests that are not already there.
pub fn write_scaffold(dir: &Path, project: &str) -> Result<Vec<PathBuf>> {
    let mut written = Vec::new();
    for (name, validator) in STARTER {
        if let Some(path) = write_one(dir, validator, name, project)? {
            written.push(path);
        }
    }
    Ok(written)
}

/// Write one test file. `file_name` may omit `.json`. Returns `None` when the file already exists.
pub fn write_one(
    dir: &Path,
    validator: &str,
    file_name: &str,
    project: &str,
) -> Result<Option<PathBuf>> {
    let body = scaffold_body(validator, project)
        .with_context(|| format!("unknown test type '{validator}'"))?;
    let relative = json_name(file_name)?;
    std::fs::create_dir_all(dir).with_context(|| format!("cannot create {}", dir.display()))?;
    let path = dir.join(&relative);
    if let Some(parent) = path.parent() {
        if parent != dir {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("cannot create {}", parent.display()))?;
        }
    }
    if path.exists() {
        return Ok(None);
    }
    std::fs::write(&path, body).with_context(|| format!("cannot write {}", path.display()))?;
    Ok(Some(path))
}

fn json_name(file_name: &str) -> Result<String> {
    let stem = file_name.strip_suffix(".json").unwrap_or(file_name);
    if stem.is_empty() {
        bail!("empty test name");
    }
    let path = Path::new(stem);
    if path.is_absolute()
        || path
            .components()
            .any(|part| matches!(part, Component::ParentDir))
    {
        bail!("test name must stay inside the test directory");
    }
    Ok(format!("{stem}.json"))
}

/// Path of `to_file` relative to `from_dir`, using `..` for parents.
pub fn relative_path(from_dir: &Path, to_file: &Path) -> String {
    let from = absolute(from_dir);
    let to = absolute(to_file);
    let from_parts = normalize(&from);
    let to_parts = normalize(&to);
    let mut shared = 0;
    while shared < from_parts.len()
        && shared < to_parts.len()
        && from_parts[shared] == to_parts[shared]
    {
        shared += 1;
    }
    let mut out = PathBuf::new();
    for _ in shared..from_parts.len() {
        out.push("..");
    }
    for part in &to_parts[shared..] {
        out.push(part);
    }
    if out.as_os_str().is_empty() {
        return ".".into();
    }
    out.to_string_lossy().replace('\\', "/")
}

fn absolute(path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(path)
    }
}

fn normalize(path: &Path) -> Vec<String> {
    let mut parts = Vec::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                parts.pop();
            }
            Component::Normal(text) => parts.push(text.to_string_lossy().into_owned()),
            Component::RootDir => parts.push(String::new()),
            Component::Prefix(prefix) => {
                parts.push(prefix.as_os_str().to_string_lossy().into_owned())
            }
        }
    }
    parts
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn project_field(text: &str) -> Option<String> {
        let value: Value = serde_json::from_str(text).ok()?;
        value
            .pointer("/spec/project")
            .and_then(Value::as_str)
            .map(str::to_string)
    }

    #[test]
    fn init_writes_erc_and_drc_once() {
        let dir = std::env::temp_dir().join(format!(
            "pcbcraft-init-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let project = "../../radio.kicad_pro";
        let created = write_scaffold(&dir, project).unwrap();
        assert_eq!(created.len(), 3);
        let erc = std::fs::read_to_string(dir.join("erc-test.json")).unwrap();
        let drc = std::fs::read_to_string(dir.join("drc-test.json")).unwrap();
        let availability = std::fs::read_to_string(dir.join("availability-test.json")).unwrap();
        assert!(erc.contains("ERCValidator"));
        assert!(drc.contains("DRCValidator"));
        assert!(availability.contains("PartAvailability"));
        assert!(availability.contains("defaultMinimum"));
        assert!(availability.contains("10"));
        assert_eq!(project_field(&erc).as_deref(), Some(project));
        assert!(write_scaffold(&dir, "other.kicad_pro").unwrap().is_empty());
        assert_eq!(project_field(&erc).as_deref(), Some(project));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn relative_path_climbs_out_of_the_test_directory() {
        let from = Path::new("/boards/radio/tests/pcbcraft");
        let to = Path::new("/boards/radio/radio.kicad_pro");
        assert_eq!(relative_path(from, to), "../../radio.kicad_pro");
    }

    #[test]
    fn a_named_type_writes_one_file_and_skips_the_second_time() {
        let dir = std::env::temp_dir().join(format!(
            "pcbcraft-one-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let created = write_one(&dir, "ResistorPower", "hot-check", "board.kicad_pro")
            .unwrap()
            .unwrap();
        let text = std::fs::read_to_string(&created).unwrap();
        assert!(text.contains("ResistorPower"));
        assert!(text.contains("rails"));
        assert!(
            write_one(&dir, "ResistorPower", "hot-check.json", "other.kicad_pro")
                .unwrap()
                .is_none()
        );
        assert!(write_one(&dir, "NoSuch", "x", "board.kicad_pro").is_err());
        assert!(json_name("../escape").is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
