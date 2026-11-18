use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use pcbcraft_domain::net::{Component, NetGraph};
use pcbcraft_domain::project::Project;
use sha2::{Digest, Sha256};

use super::board::track_widths;
use super::build::assemble;
use super::cache::NetCache;
use super::schematic::components_and_sheets;
use crate::command::KiCadCli;
use crate::netlist::parse_design;

pub fn load_graph(cli: &KiCadCli, project: &Project) -> Result<NetGraph> {
    let schematic = project
        .schematic
        .as_ref()
        .context("project has no schematic")?;
    let board = project.board.as_ref().context("project has no board")?;
    let sheets = collect_sheets(schematic)?;
    let mut files = sheets;
    files.push(board.clone());
    let mut hashes = BTreeMap::new();
    for path in &files {
        hashes.insert(path.display().to_string(), hash_file(path)?);
    }
    let mut cache = NetCache::open(&project.net_cache())?;
    if let Some(graph) = cache.load_if_fresh(&hashes)? {
        return Ok(graph);
    }
    let mut components = Vec::new();
    for path in files
        .iter()
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("kicad_sch"))
    {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("cannot read {}", path.display()))?;
        let (found, _) = components_and_sheets(&text)?;
        merge_components(&mut components, found);
    }
    let dir = std::env::temp_dir().join(format!("pcbcraft-graph-{}", std::process::id()));
    std::fs::create_dir_all(&dir)?;
    let net_path = dir.join("net.xml");
    cli.run(&[
        "sch".into(),
        "export".into(),
        "netlist".into(),
        "--format".into(),
        "kicadxml".into(),
        "--output".into(),
        net_path.display().to_string(),
        schematic.display().to_string(),
    ])?;
    let xml = std::fs::read_to_string(&net_path)?;
    let (_design, groups) = parse_design(&xml)?;
    let board_text = std::fs::read_to_string(board)
        .with_context(|| format!("cannot read {}", board.display()))?;
    let widths = track_widths(&board_text)?;
    let graph = assemble(components, groups, &widths);
    cache.store(&hashes, &graph)?;
    Ok(graph)
}

pub(crate) fn collect_sheets(root: &Path) -> Result<Vec<PathBuf>> {
    let mut pending = vec![root.to_path_buf()];
    let mut seen = Vec::new();
    while let Some(path) = pending.pop() {
        if seen.iter().any(|seen: &PathBuf| seen == &path) {
            continue;
        }
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("cannot read {}", path.display()))?;
        let (_, sheets) = components_and_sheets(&text)?;
        seen.push(path.clone());
        let dir = path.parent().unwrap_or_else(|| Path::new("."));
        for sheet in sheets {
            pending.push(dir.join(sheet.file));
        }
    }
    seen.sort();
    Ok(seen)
}

fn merge_components(into: &mut Vec<Component>, found: Vec<Component>) {
    for component in found {
        if into
            .iter()
            .any(|have| have.reference == component.reference)
        {
            continue;
        }
        into.push(component);
    }
}

fn hash_file(path: &Path) -> Result<String> {
    let bytes = std::fs::read(path).with_context(|| format!("cannot read {}", path.display()))?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}
