//! `pcbcraft geometry` subcommands. Parsing lives in the KiCad crate.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use clap::{Args, Subcommand};
use pcbcraft_application::geometry::{
    dump_json, dump_view, find_symbol, occupied_json, DumpOptions, OccupiedOptions,
};
use pcbcraft_domain::geometry::{parse_window, GeometryError, Point};
use pcbcraft_kicad::geometry::{load_cached, read_clearance_rules};
use pcbcraft_kicad::project::{require_file, resolve};
use pcbcraft_render::geometry_png;

#[derive(Subcommand)]
pub enum GeometryKind {
    /// Outlines on the whole board, or inside a window.
    Dump(DumpArgs),
    /// Whether a point is inside a blocking area, and how far the free space reaches.
    IsOccupied(OccupiedArgs),
    /// Shapes of one footprint placed on the board.
    FindSymbol(FindSymbolArgs),
}

#[derive(Args)]
pub struct DumpArgs {
    /// Window `XMIN-XMAXxYMIN-YMAX`, for example `0-50x0-40`. Omit it to dump the whole board.
    window: Option<String>,
    /// Include silk, fab, mask, paste, and text.
    #[arg(long)]
    all: bool,
    /// Add a sampled `points` list beside each outline.
    #[arg(long)]
    detailed: bool,
    /// Sampling step in millimetres for `--detailed`.
    #[arg(long, default_value_t = 0.01)]
    detailed_precision: f64,
    /// `json` prints the outlines. `png` draws them.
    #[arg(long, value_parser = ["json", "png"], default_value = "json")]
    format: String,
    /// Output file. JSON defaults to stdout. PNG defaults to pcb-geometry.png.
    #[arg(long)]
    output: Option<PathBuf>,
    #[arg(long)]
    board: Option<PathBuf>,
    /// A .kicad_pro, .kicad_sch, or .kicad_pcb file.
    input: Option<PathBuf>,
}

#[derive(Args)]
pub struct OccupiedArgs {
    #[arg(long)]
    x: f64,
    #[arg(long)]
    y: f64,
    /// Side of the square around the point, in millimetres.
    #[arg(long, default_value_t = 10.0)]
    size: f64,
    #[arg(long)]
    detailed: bool,
    #[arg(long, default_value_t = 0.01)]
    detailed_precision: f64,
    #[arg(long)]
    board: Option<PathBuf>,
    /// A .kicad_pro, .kicad_sch, or .kicad_pcb file.
    input: Option<PathBuf>,
}

#[derive(Args)]
pub struct FindSymbolArgs {
    /// Footprint reference, for example U1.
    reference: String,
    #[arg(long)]
    board: Option<PathBuf>,
    /// A .kicad_pro, .kicad_sch, or .kicad_pcb file.
    input: Option<PathBuf>,
}

pub fn run(kind: GeometryKind) -> Result<()> {
    let value = match kind {
        GeometryKind::Dump(args) => {
            let format = args.format.clone();
            let output = args.output.clone();
            let (window, input) = dump_target(args.window, args.input)?;
            let board = load(&input, args.board.as_deref())?;
            let options = DumpOptions {
                all: args.all,
                detailed: args.detailed,
                precision: args.detailed_precision,
            };
            return write_dump(&board, window, options, &format, output.as_deref());
        }
        GeometryKind::IsOccupied(args) => {
            if args.size <= 0.0 {
                bail!("size must be positive");
            }
            let board = load(&args.input, args.board.as_deref())?;
            occupied_json(
                &board,
                Point::new(args.x, args.y),
                OccupiedOptions {
                    size: args.size,
                    detailed: args.detailed,
                    precision: args.detailed_precision,
                },
            )
            .map_err(anyhow_geometry)?
        }
        GeometryKind::FindSymbol(args) => {
            let board = load(&args.input, args.board.as_deref())?;
            find_symbol(&board, &args.reference).map_err(anyhow_geometry)?
        }
    };
    println!("{}", serde_json::to_string_pretty(&value)?);
    Ok(())
}

fn write_dump(
    board: &pcbcraft_domain::geometry::BoardGeometry,
    window: Option<pcbcraft_domain::geometry::Window>,
    options: DumpOptions,
    format: &str,
    output: Option<&Path>,
) -> Result<()> {
    let view = dump_view(board, window, options).map_err(anyhow_geometry)?;
    match format {
        "png" => {
            let path = match dump_destination(format, output)? {
                DumpDestination::Stdout => bail!("png output cannot be stdout"),
                DumpDestination::File(path) => path,
            };
            let bytes = geometry_png(
                &view.shapes,
                view.window,
                options.detailed,
                options.precision,
            )?;
            write_bytes(&path, &bytes)?;
            eprintln!("Wrote {}", path.display());
        }
        _ => {
            let value = dump_json(board, window, options).map_err(anyhow_geometry)?;
            let text = serde_json::to_string_pretty(&value)?;
            match dump_destination(format, output)? {
                DumpDestination::Stdout => println!("{text}"),
                DumpDestination::File(path) => {
                    write_bytes(&path, text.as_bytes())?;
                    eprintln!("Wrote {}", path.display());
                }
            }
        }
    }
    Ok(())
}

enum DumpDestination {
    Stdout,
    File(PathBuf),
}

fn dump_destination(format: &str, output: Option<&Path>) -> Result<DumpDestination> {
    match (format, output) {
        ("png", Some(path)) if path == Path::new("-") => {
            bail!("png output cannot be stdout")
        }
        ("png", Some(path)) => Ok(DumpDestination::File(path.to_path_buf())),
        ("png", None) => Ok(DumpDestination::File(PathBuf::from("pcb-geometry.png"))),
        (_, Some(path)) if path == Path::new("-") => Ok(DumpDestination::Stdout),
        (_, Some(path)) => Ok(DumpDestination::File(path.to_path_buf())),
        (_, None) => Ok(DumpDestination::Stdout),
    }
}

fn write_bytes(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }
    fs::write(path, bytes).with_context(|| format!("cannot write {}", path.display()))
}

fn dump_target(
    window: Option<String>,
    input: Option<PathBuf>,
) -> Result<(Option<pcbcraft_domain::geometry::Window>, Option<PathBuf>)> {
    let Some(text) = window else {
        return Ok((None, input));
    };
    match parse_window(&text) {
        Ok(window) => Ok((Some(window), input)),
        Err(_) if input.is_none() && looks_like_project(&text) => {
            Ok((None, Some(PathBuf::from(text))))
        }
        Err(err) => Err(anyhow_window(err)),
    }
}

fn looks_like_project(text: &str) -> bool {
    let path = Path::new(text);
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| matches!(ext, "kicad_pcb" | "kicad_pro" | "kicad_sch"))
        || path.exists()
}

pub fn load_geometry(
    input: Option<&Path>,
    board: Option<&Path>,
) -> Result<pcbcraft_domain::geometry::BoardGeometry> {
    load(&input.map(Path::to_path_buf), board)
}

fn load(
    input: &Option<PathBuf>,
    board: Option<&Path>,
) -> Result<pcbcraft_domain::geometry::BoardGeometry> {
    let project = resolve(input.as_deref(), None, board)?;
    let board_path = project.board.clone().context("project has no board")?;
    require_file(&board_path, "board")?;
    let pro = project_file(&project, &board_path)?;
    let rules_text =
        fs::read_to_string(&pro).with_context(|| format!("cannot read {}", pro.display()))?;
    let rules = read_clearance_rules(&rules_text)?;
    load_cached(&project.cache_dir(), &board_path, &rules)
}

fn project_file(project: &pcbcraft_domain::project::Project, board: &Path) -> Result<PathBuf> {
    if let Some(pro) = &project.pro {
        require_file(pro, "project")?;
        return Ok(pro.clone());
    }
    let stem = board
        .file_stem()
        .context("board file name")?
        .to_string_lossy();
    let sibling = board
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(format!("{stem}.kicad_pro"));
    if sibling.is_file() {
        return Ok(sibling);
    }
    bail!(
        "clearance rules are missing: no .kicad_pro beside {}",
        board.display()
    )
}

fn anyhow_window(err: GeometryError) -> anyhow::Error {
    anyhow::anyhow!("{err}")
}

fn anyhow_geometry(err: GeometryError) -> anyhow::Error {
    anyhow::anyhow!("{err}")
}

#[cfg(test)]
mod tests {
    use super::{dump_destination, DumpDestination};
    use std::path::Path;

    #[test]
    fn json_defaults_to_stdout_and_png_defaults_to_the_picture_file() {
        assert!(matches!(
            dump_destination("json", None).unwrap(),
            DumpDestination::Stdout
        ));
        match dump_destination("png", None).unwrap() {
            DumpDestination::File(path) => assert_eq!(path, Path::new("pcb-geometry.png")),
            DumpDestination::Stdout => panic!("png must be a file"),
        }
        assert!(dump_destination("png", Some(Path::new("-"))).is_err());
        match dump_destination("png", Some(Path::new("board.png"))).unwrap() {
            DumpDestination::File(path) => assert_eq!(path, Path::new("board.png")),
            DumpDestination::Stdout => panic!("named png must be a file"),
        }
    }
}
