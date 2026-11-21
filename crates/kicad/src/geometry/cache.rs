//! Full-board geometry cache next to the project.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use pcbcraft_domain::geometry::{apply_clearance, BoardGeometry, ClearanceRules};
use sha2::{Digest, Sha256};

use super::binary::{decode, encode};
use super::parse_board;

/// Load the whole board from `.pcbcraft/geometry`, rebuilding when the pcb file hash differs.
pub fn load_cached(
    cache_dir: &Path,
    board_path: &Path,
    rules: &ClearanceRules,
) -> Result<BoardGeometry> {
    let name = cache_name(board_path);
    let cache_path = cache_dir.join("geometry").join(format!("{name}.bin"));
    let hash_path = cache_dir.join("geometry").join(format!("{name}-hash"));
    let hash = file_hash(board_path, rules)?;
    if let Some(geometry) = read_fresh(&cache_path, &hash_path, &hash)? {
        return Ok(geometry);
    }
    let text = fs::read_to_string(board_path)
        .with_context(|| format!("cannot read {}", board_path.display()))?;
    let mut geometry = parse_board(&text)?;
    apply_clearance(&mut geometry, rules);
    write_cache(&cache_path, &hash_path, &hash, &geometry)?;
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

fn read_fresh(cache_path: &Path, hash_path: &Path, hash: &str) -> Result<Option<BoardGeometry>> {
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
    let bytes = match fs::read(cache_path) {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => {
            return Err(err).with_context(|| format!("cannot read {}", cache_path.display()));
        }
    };
    // A cache that does not decode is rebuilt, never an error.
    Ok(decode(&bytes))
}

fn write_cache(
    cache_path: &Path,
    hash_path: &Path,
    hash: &str,
    geometry: &BoardGeometry,
) -> Result<()> {
    for path in [cache_path, hash_path] {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("cannot create {}", parent.display()))?;
        }
    }
    // Older versions kept a JSON cache, and its hash one directory up.
    let _ = fs::remove_file(cache_path.with_extension("json"));
    if let (Some(dir), Some(name)) = (
        hash_path.parent().and_then(Path::parent),
        hash_path.file_name(),
    ) {
        let _ = fs::remove_file(dir.join(name));
    }
    write_new(cache_path, &encode(geometry))?;
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

/// Bump when the parser or the stored shape changes, so old caches are rebuilt.
const CACHE_VERSION: u32 = 3;

/// The cache holds clearance halos, so the key covers the board and the rules.
fn file_hash(path: &Path, rules: &ClearanceRules) -> Result<String> {
    let bytes = fs::read(path).with_context(|| format!("cannot read {}", path.display()))?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    hasher.update(format!("\ncache-v{CACHE_VERSION}\n{rules:?}").as_bytes());
    Ok(format!("{:x}", hasher.finalize()))
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

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("pcbcraft-geometry-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_matching_hash_reuses_the_cache_and_a_new_file_rebuilds_it() {
        let dir = scratch("cache");
        let board = dir.join("My Board.kicad_pcb");
        fs::write(&board, BOARD).unwrap();
        let cache = dir.join(".pcbcraft");
        let first = load_cached(&cache, &board, &rules()).unwrap();
        let name = cache_name(&board);
        assert_eq!(name, "My-Board.kicad_pcb");
        let cache_path = cache.join("geometry").join(format!("{name}.bin"));
        let hash_path = cache.join("geometry").join(format!("{name}-hash"));
        assert!(cache_path.is_file());
        let stored_hash = fs::read_to_string(&hash_path).unwrap();
        assert_eq!(stored_hash.trim(), file_hash(&board, &rules()).unwrap());

        // Swap in a cache without the graphics. It is used while the hash matches.
        let mut trimmed = first.clone();
        trimmed
            .shapes
            .retain(|shape| shape.source != Source::Graphic);
        fs::write(&cache_path, encode(&trimmed)).unwrap();
        let second = load_cached(&cache, &board, &rules()).unwrap();
        assert_eq!(second, trimmed);
        assert_eq!(first.shapes.len(), second.shapes.len() + 1);
        assert_eq!(fs::read_to_string(&hash_path).unwrap(), stored_hash);
        let track = second
            .shapes
            .iter()
            .find(|shape| shape.source == Source::Track)
            .unwrap();
        assert!(point_blocked(track, Point::new(5.0, 0.2)));

        fs::write(&board, format!("{BOARD}\n")).unwrap();
        let third = load_cached(&cache, &board, &rules()).unwrap();
        assert_eq!(third, first);
        assert_ne!(
            fs::read_to_string(&hash_path).unwrap().trim(),
            stored_hash.trim()
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn changed_rules_and_a_damaged_cache_both_rebuild() {
        let dir = scratch("rebuild");
        let board = dir.join("b.kicad_pcb");
        fs::write(&board, BOARD).unwrap();
        let cache = dir.join(".pcbcraft");
        let first = load_cached(&cache, &board, &rules()).unwrap();
        let mut wider = rules();
        wider.classes[0].clearance = 1.0;
        let second = load_cached(&cache, &board, &wider).unwrap();
        assert_ne!(first, second);
        let cache_path = cache.join("geometry").join("b.kicad_pcb.bin");
        let bytes = fs::read(&cache_path).unwrap();
        fs::write(&cache_path, &bytes[..bytes.len() / 2]).unwrap();
        assert_eq!(load_cached(&cache, &board, &wider).unwrap(), second);
        fs::write(&cache_path, b"not a cache").unwrap();
        assert_eq!(load_cached(&cache, &board, &wider).unwrap(), second);
        // The healed file is a good cache again.
        assert_eq!(decode(&fs::read(&cache_path).unwrap()).unwrap(), second);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_binary_form_keeps_every_field_of_a_board() {
        let mut board = parse_board(include_str!("binary_sample.kicad_pcb")).unwrap();
        apply_clearance(&mut board, &rules());
        assert!(board.shapes.iter().any(|shape| shape.pad_form.is_some()));
        assert!(!board.copper_layers.is_empty());
        assert!(!board.footprints.is_empty());
        let bytes = encode(&board);
        assert_eq!(decode(&bytes).unwrap(), board);
        // No prefix of the bytes may decode, and none may panic.
        for length in (0..bytes.len()).step_by(7) {
            assert!(decode(&bytes[..length]).is_none(), "{length}");
        }
        let mut flipped = bytes.clone();
        for index in (0..flipped.len()).step_by(11) {
            flipped[index] ^= 0xff;
            let _ = decode(&flipped);
            flipped[index] ^= 0xff;
        }
    }
}
