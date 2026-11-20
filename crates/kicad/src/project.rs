use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use pcbcraft_domain::project::Project;

pub fn resolve(
    input: Option<&Path>,
    schematic: Option<&Path>,
    board: Option<&Path>,
) -> Result<Project> {
    let base = std::env::current_dir().context("current directory")?;
    resolve_in(&base, input, schematic, board)
}

/// Resolve a project using `base` when no input file is given.
/// Relative paths are joined onto `base`. This does not change the process directory.
pub fn resolve_in(
    base: &Path,
    input: Option<&Path>,
    schematic: Option<&Path>,
    board: Option<&Path>,
) -> Result<Project> {
    let input = input.map(|path| join_base(base, path));
    let schematic = schematic.map(|path| join_base(base, path));
    let board = board.map(|path| join_base(base, path));
    resolve_anchored(
        base,
        input.as_deref(),
        schematic.as_deref(),
        board.as_deref(),
    )
}

fn join_base(base: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        base.join(path)
    }
}

fn resolve_anchored(
    base: &Path,
    input: Option<&Path>,
    schematic: Option<&Path>,
    board: Option<&Path>,
) -> Result<Project> {
    if schematic.is_some() || board.is_some() {
        let directory = schematic
            .or(board)
            .or(input)
            .and_then(|path| path.parent())
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf();
        return Ok(Project {
            directory,
            pro: input
                .filter(|path| is_ext(path, "kicad_pro"))
                .map(Path::to_path_buf),
            schematic: schematic.map(Path::to_path_buf),
            board: board.map(Path::to_path_buf),
        });
    }
    let input = match input {
        Some(path) => path.to_path_buf(),
        None => only_project(base)?,
    };
    if is_ext(&input, "kicad_pro") {
        let directory = parent_of(&input);
        let stem = input
            .file_stem()
            .context("nazwa projektu")?
            .to_string_lossy()
            .into_owned();
        return Ok(Project {
            pro: Some(input.clone()),
            schematic: Some(directory.join(format!("{stem}.kicad_sch"))),
            board: Some(directory.join(format!("{stem}.kicad_pcb"))),
            directory,
        });
    }
    if is_ext(&input, "kicad_sch") {
        let directory = parent_of(&input);
        let stem = input.file_stem().unwrap().to_string_lossy().into_owned();
        return Ok(Project {
            schematic: Some(input),
            board: Some(directory.join(format!("{stem}.kicad_pcb"))),
            pro: None,
            directory,
        });
    }
    if is_ext(&input, "kicad_pcb") {
        let directory = parent_of(&input);
        return Ok(Project {
            directory: directory.clone(),
            pro: None,
            schematic: None,
            board: Some(input),
        });
    }
    bail!("expected a .kicad_pro, .kicad_sch, or .kicad_pcb file");
}

fn parent_of(path: &Path) -> PathBuf {
    path.parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf()
}

fn is_ext(path: &Path, ext: &str) -> bool {
    path.extension().and_then(|ext_| ext_.to_str()) == Some(ext)
}

fn only_project(dir: &Path) -> Result<PathBuf> {
    let mut found = Vec::new();
    for entry in std::fs::read_dir(dir).with_context(|| format!("{}", dir.display()))? {
        let entry = entry?;
        if is_ext(&entry.path(), "kicad_pro") {
            found.push(entry.path());
        }
    }
    match found.len() {
        1 => Ok(found.remove(0)),
        0 => bail!("no .kicad_pro in {}", dir.display()),
        _ => bail!("more than one .kicad_pro in {}", dir.display()),
    }
}

pub fn require_file(path: &Path, label: &str) -> Result<()> {
    if path.is_file() {
        Ok(())
    } else {
        bail!("missing {label}: {}", path.display())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_project_file_selects_its_siblings() {
        let dir = std::env::temp_dir().join(format!("pcbcraft-proj-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let pro = dir.join("radio.kicad_pro");
        std::fs::write(&pro, "{}").unwrap();
        let project = resolve(Some(&pro), None, None).unwrap();
        assert_eq!(project.schematic.unwrap(), dir.join("radio.kicad_sch"));
        assert_eq!(project.board.unwrap(), dir.join("radio.kicad_pcb"));
        assert!(resolve(Some(&dir.join("notes.txt")), None, None).is_err());
        assert!(require_file(&dir.join("missing"), "board").is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_empty_directory_has_no_project() {
        let dir = std::env::temp_dir().join(format!("pcbcraft-empty-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let previous = std::env::current_dir().unwrap();
        std::env::set_current_dir(&dir).unwrap();
        let missing = resolve(None, None, None);
        std::env::set_current_dir(previous).unwrap();
        assert!(missing.is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
