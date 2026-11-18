//! Full-board geometry cache next to the project.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use pcbcraft_application::geometry::{board_from_dump, dump_json, DumpOptions};
use pcbcraft_domain::geometry::{apply_clearance, BoardGeometry, ClearanceRules, GeometryError};
use sha2::{Digest, Sha256};

use super::parse_board;

/// Load the whole board from `.pcbcraft`, rebuilding when the pcb file hash differs.
pub fn load_cached(
    cache_dir: &Path,
    board_path: &Path,
    rules: &ClearanceRules,
) -> Result<BoardGeometry> {
    let name = cache_name(board_path);
    let json_path = cache_dir.join("geometry").join(format!("{name}.json"));
    let hash_path = cache_dir.join(format!("{name}-hash"));
    let hash = file_hash(board_path)?;
    if let Some(geometry) = read_fresh(&json_path, &hash_path, &hash)? {
        return Ok(geometry);
    }
    let text = fs::read_to_string(board_path)
        .with_context(|| format!("cannot read {}", board_path.display()))?;
    let mut geometry = parse_board(&text)?;
    apply_clearance(&mut geometry, rules);
    write_cache(&json_path, &hash_path, &hash, &geometry)?;
    Ok(geometry)
}

pub fn cache_name(board_path: &Path) -> String {
    let name = board_path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "board.kicad_pcb".into());
    name.chars()
        .map(|symbol| {
            if symbol.is_ascii_alphanumeric() || matches!(symbol, '.' | '_' | '-') {
                symbol
            } else {
                '-'
            }
        })
        .collect()
}

fn read_fresh(json_path: &Path, hash_path: &Path, hash: &str) -> Result<Option<BoardGeometry>> {
    let stored = match fs::read_to_string(hash_path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => {
            return Err(err).with_context(|| format!("cannot read {}", hash_path.display()));
        }
    };
    if stored.trim() != hash {
        return Ok(None);
    }
    let text = match fs::read_to_string(json_path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => {
            return Err(err).with_context(|| format!("cannot read {}", json_path.display()));
        }
    };
    let value = match serde_json::from_str(&text) {
        Ok(value) => value,
        Err(_) => return Ok(None),
    };
    match board_from_dump(&value) {
        Ok(geometry) => Ok(Some(geometry)),
        Err(GeometryError::BadCache) => Ok(None),
        Err(err) => Err(err.into()),
    }
}

fn write_cache(
    json_path: &Path,
    hash_path: &Path,
    hash: &str,
    geometry: &BoardGeometry,
) -> Result<()> {
    if let Some(parent) = json_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("cannot create {}", parent.display()))?;
    }
    if let Some(parent) = hash_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("cannot create {}", parent.display()))?;
    }
    let value = dump_json(
        geometry,
        None,
        DumpOptions {
            all: true,
            detailed: false,
            precision: 0.01,
        },
    )?;
    let body = serde_json::to_string_pretty(&value)?;
    write_new(json_path, body.as_bytes())?;
    write_new(hash_path, format!("{hash}\n").as_bytes())?;
    Ok(())
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<()> {
    let tmp = temp_beside(path);
    fs::write(&tmp, bytes).with_context(|| format!("cannot write {}", tmp.display()))?;
    fs::rename(&tmp, path).with_context(|| format!("cannot replace {}", path.display()))?;
    Ok(())
}

fn temp_beside(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "geometry".into());
    path.with_file_name(format!(".{name}.{}.tmp", std::process::id()))
}

fn file_hash(path: &Path) -> Result<String> {
    let bytes = fs::read(path).with_context(|| format!("cannot read {}", path.display()))?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pcbcraft_domain::geometry::{point_blocked, Point, Source};

    const BOARD: &str = r#"
(kicad_pcb
  (layers (0 "F.Cu" signal))
  (segment (start 0 0) (end 10 0) (width 0.25) (layer "F.Cu") (net "GND"))
  (gr_line (start 0 2) (end 1 2) (stroke (width 0.1)) (layer "F.SilkS"))
)
"#;

    fn rules() -> ClearanceRules {
        ClearanceRules {
            min_clearance: 0.15,
            min_hole_clearance: 0.25,
            min_copper_edge_clearance: 0.5,
            classes: vec![pcbcraft_domain::geometry::NetClass {
                name: "Default".into(),
                clearance: 0.15,
                priority: 1,
            }],
            patterns: Vec::new(),
        }
    }

    #[test]
    fn a_matching_hash_reuses_the_json_and_a_new_file_rebuilds_it() {
        let dir = std::env::temp_dir().join(format!(
            "pcbcraft-geometry-cache-{}-{}",
            std::process::id(),
            "board"
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let board = dir.join("My Board.kicad_pcb");
        fs::write(&board, BOARD).unwrap();
        let cache = dir.join(".pcbcraft");
        let first = load_cached(&cache, &board, &rules()).unwrap();
        let name = cache_name(&board);
        assert_eq!(name, "My-Board.kicad_pcb");
        let json_path = cache.join("geometry").join(format!("{name}.json"));
        let hash_path = cache.join(format!("{name}-hash"));
        assert!(json_path.is_file());
        let stored_hash = fs::read_to_string(&hash_path).unwrap();
        assert_eq!(stored_hash.trim(), file_hash(&board).unwrap());
        let mut cached: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&json_path).unwrap()).unwrap();
        cached["layers"]["F.SilkS"] = serde_json::json!([]);
        fs::write(&json_path, serde_json::to_string(&cached).unwrap()).unwrap();
        let second = load_cached(&cache, &board, &rules()).unwrap();
        assert!(second
            .shapes
            .iter()
            .all(|shape| shape.source != Source::Graphic));
        assert_eq!(fs::read_to_string(&hash_path).unwrap(), stored_hash);
        let track = second
            .shapes
            .iter()
            .find(|shape| shape.source == Source::Track)
            .unwrap();
        assert!(point_blocked(track, Point::new(5.0, 0.2)));
        assert_eq!(first.shapes.len(), second.shapes.len() + 1);

        fs::write(&board, format!("{BOARD}\n")).unwrap();
        let third = load_cached(&cache, &board, &rules()).unwrap();
        assert!(third
            .shapes
            .iter()
            .any(|shape| shape.source == Source::Graphic && !shape.blocks));
        assert_eq!(
            fs::read_to_string(&hash_path).unwrap().trim(),
            file_hash(&board).unwrap()
        );
        assert_ne!(
            fs::read_to_string(&hash_path).unwrap().trim(),
            stored_hash.trim()
        );
        let _ = fs::remove_dir_all(&dir);
    }
}
