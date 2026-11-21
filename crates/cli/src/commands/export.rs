use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use pcbcraft_application::export::{correct_position, mechanical_page, rewrite_comments};
use pcbcraft_application::ports::{Fabrication, RotationDb};
use pcbcraft_domain::drawing::Side;
use pcbcraft_kicad::dxf::parse_outline;
use pcbcraft_kicad::excellon;
use pcbcraft_kicad::gerbers::zip_dir;
use pcbcraft_kicad::project::{require_file, resolve_in};
use pcbcraft_kicad::rotation_db::{read_overlay, CachedRotationDb};
use pcbcraft_kicad::KiCad;
use pcbcraft_render::PdfRenderer;

use super::args::{CplArgs, DrawingArgs, FileArgs, TwoDArgs};
use super::output::{place, scratch, CommandOutput};
use crate::geometry_cmd::load_geometry;

pub fn bom(cwd: &Path, args: &FileArgs) -> CommandOutput {
    CommandOutput::from_result(bom_result(cwd, args))
}

pub fn pos(cwd: &Path, args: &FileArgs) -> CommandOutput {
    CommandOutput::from_result(pos_result(cwd, args))
}

pub fn cpl(cwd: &Path, args: &CplArgs) -> CommandOutput {
    CommandOutput::from_result(cpl_result(cwd, args))
}

pub fn gerbers(cwd: &Path, args: &FileArgs) -> CommandOutput {
    CommandOutput::from_result(gerbers_result(cwd, args))
}

pub fn drawing(cwd: &Path, args: &DrawingArgs) -> CommandOutput {
    CommandOutput::from_result(drawing_result(cwd, args))
}

pub fn two_d(cwd: &Path, args: &TwoDArgs) -> CommandOutput {
    CommandOutput::from_result(two_d_result(cwd, args))
}

fn bom_result(cwd: &Path, args: &FileArgs) -> Result<CommandOutput> {
    let project = resolve_in(
        cwd,
        args.input.as_deref(),
        args.schematic.as_deref(),
        args.board.as_deref(),
    )?;
    let schematic = project.schematic.as_ref().context("no schematic")?;
    require_file(schematic, "schematic")?;
    let output = place(
        cwd,
        &args
            .output
            .clone()
            .unwrap_or_else(|| PathBuf::from(format!("{}-bom.csv", project.stem()))),
    );
    let temp = scratch("bom").join("bom.csv");
    KiCad::from_env().export_bom(schematic, &temp)?;
    let text = fs::read_to_string(&temp)?;
    write_out(&output, &rewrite_comments(&text)?)
}

fn pos_result(cwd: &Path, args: &FileArgs) -> Result<CommandOutput> {
    let project = resolve_in(
        cwd,
        args.input.as_deref(),
        args.schematic.as_deref(),
        args.board.as_deref(),
    )?;
    let board = project.board.as_ref().context("no board")?;
    require_file(board, "board")?;
    let output = place(
        cwd,
        &args
            .output
            .clone()
            .unwrap_or_else(|| PathBuf::from(format!("{}-pos.csv", project.stem()))),
    );
    KiCad::from_env().export_pos(board, &output)?;
    Ok(CommandOutput::note(format!("Wrote {}\n", output.display())))
}

fn cpl_result(cwd: &Path, args: &CplArgs) -> Result<CommandOutput> {
    let input = args.input.as_ref().map(|path| place(cwd, path));
    let input_is_csv = input
        .as_ref()
        .is_some_and(|path| path.extension().and_then(|ext| ext.to_str()) == Some("csv"));
    let (pos_text, bom_text, stem) = if input_is_csv {
        let path = input.as_ref().unwrap();
        let bom = args
            .bom
            .as_ref()
            .map(|path| fs::read_to_string(place(cwd, path)))
            .transpose()?;
        (
            fs::read_to_string(path)?,
            bom,
            path.file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
                .unwrap_or_else(|| "positions".into()),
        )
    } else {
        let project = resolve_in(
            cwd,
            args.input.as_deref(),
            args.schematic.as_deref(),
            args.board.as_deref(),
        )?;
        let schematic = project.schematic.as_ref().context("no schematic")?;
        let board = project.board.as_ref().context("no board")?;
        require_file(schematic, "schematic")?;
        require_file(board, "board")?;
        let dir = scratch("cpl");
        let kicad = KiCad::from_env();
        let bom_path = dir.join("bom.csv");
        let pos_path = dir.join("pos.csv");
        kicad.export_bom(schematic, &bom_path)?;
        kicad.export_pos(board, &pos_path)?;
        let bom_raw = fs::read_to_string(&bom_path)?;
        (
            fs::read_to_string(&pos_path)?,
            Some(rewrite_comments(&bom_raw)?),
            project.stem(),
        )
    };
    let db = CachedRotationDb {
        cache_dir: args.cache_dir.as_ref().map(|path| place(cwd, path)),
        explicit: args.db.as_ref().map(|path| place(cwd, path)),
    };
    let database = db.load(args.update)?;
    let local = args
        .local
        .as_ref()
        .map(|path| fs::read_to_string(place(cwd, path)))
        .transpose()?;
    let mut extras = Vec::new();
    for path in &args.corrections {
        extras.push(read_overlay(&place(cwd, path))?);
    }
    let produced = correct_position(
        &pos_text,
        bom_text.as_deref().or(args.bom.as_ref().and_then(|_| None)),
        &database,
        local.as_deref(),
        &extras,
    )?;
    let mut stderr = String::new();
    if !args.quiet && !produced.report.is_empty() {
        stderr.push_str(&produced.report);
    }
    if args.dry_run {
        stderr.push_str("Not writing (dry-run).\n");
        return Ok(CommandOutput::note(stderr));
    }
    let output = place(
        cwd,
        &args
            .output
            .clone()
            .unwrap_or_else(|| PathBuf::from(format!("{stem}-cpl.csv"))),
    );
    let mut written = write_out(&output, &produced.csv)?;
    written.stderr.insert_str(0, &stderr);
    Ok(written)
}

fn gerbers_result(cwd: &Path, args: &FileArgs) -> Result<CommandOutput> {
    let project = resolve_in(
        cwd,
        args.input.as_deref(),
        args.schematic.as_deref(),
        args.board.as_deref(),
    )?;
    let board = project.board.as_ref().context("no board")?;
    require_file(board, "board")?;
    let dir = scratch("gerber");
    let kicad = KiCad::from_env();
    kicad.export_gerbers(board, &dir)?;
    kicad.export_drill_split(board, &dir)?;
    let output = place(
        cwd,
        &args
            .output
            .clone()
            .unwrap_or_else(|| PathBuf::from(format!("{}-gerbers.zip", project.stem()))),
    );
    zip_dir(&dir, &output)?;
    let _ = fs::remove_dir_all(&dir);
    Ok(CommandOutput::note(format!("Wrote {}\n", output.display())))
}

fn drawing_result(cwd: &Path, args: &DrawingArgs) -> Result<CommandOutput> {
    let project = resolve_in(cwd, args.input.as_deref(), None, args.board.as_deref())?;
    let board = project.board.as_ref().context("no board")?;
    require_file(board, "board")?;
    let sides = match &args.side {
        Some(text) => {
            let side = Side::parse(text).context("side must be side or back")?;
            vec![side]
        }
        None => Vec::new(),
    };
    let kicad = KiCad::from_env();
    let dir = scratch("draw");
    let output = place(
        cwd,
        &args
            .output
            .clone()
            .unwrap_or_else(|| PathBuf::from(format!("{}-drawing.pdf", project.stem()))),
    );
    let dxf_path = dir.join("outline.dxf");
    let drill_path = dir.join("drills.drl");
    kicad.export_dxf_outline(board, &dxf_path)?;
    kicad.export_excellon(board, &drill_path)?;
    let mut geometry = parse_outline(&fs::read_to_string(&dxf_path)?)?;
    let drills = excellon::parse(&fs::read_to_string(&drill_path)?)?;
    geometry.holes = drills.holes;
    geometry.slots = drills.slots;
    let page = mechanical_page(&sides, &geometry)?;
    if args.full {
        let faces: Vec<Side> = if sides.is_empty() {
            vec![Side::Side, Side::Back]
        } else {
            sides.clone()
        };
        let mut svgs = Vec::new();
        for side in faces {
            let path = dir.join(format!("{}.svg", side.as_str()));
            kicad.export_svg(board, side, true, &path)?;
            svgs.push((side, fs::read_to_string(&path)?));
        }
        PdfRenderer.color(&page, &svgs, &output)?;
    } else {
        PdfRenderer.mechanical(&page, &geometry, &output)?;
    }
    Ok(CommandOutput::note(format!("Wrote {}\n", output.display())))
}

fn two_d_result(cwd: &Path, args: &TwoDArgs) -> Result<CommandOutput> {
    if args.around_space.is_some() && args.symbol.is_none() {
        bail!("--around-space requires --symbol");
    }
    let project = resolve_in(cwd, args.input.as_deref(), None, args.board.as_deref())?;
    let board = project.board.as_ref().context("no board")?;
    require_file(board, "board")?;
    let requested = pcbcraft_domain::geometry::Face::parse(&args.side)
        .with_context(|| format!("--side must be front or back, not '{}'", args.side))?;
    let (frame, face) = if let Some(symbol) = &args.symbol {
        let margin = match &args.around_space {
            Some(text) => pcbcraft_domain::geometry::parse_millimetres(text)?,
            None => 5.0,
        };
        let geometry = load_geometry(cwd, args.input.as_deref(), args.board.as_deref())?;
        let (bounds, shapes) = pcbcraft_application::geometry::symbol_view(&geometry, symbol)?;
        let square = pcbcraft_domain::geometry::square_around(bounds, margin);
        let board_window = pcbcraft_domain::geometry::board_window(&geometry);
        let frame = pcbcraft_domain::geometry::render_frame(board_window, square);
        let face = pcbcraft_domain::geometry::symbol_face(&shapes);
        (Some(frame), face)
    } else {
        (None, requested)
    };
    let output = place(
        cwd,
        &args.output.clone().unwrap_or_else(|| {
            let suffix = match (&args.symbol, face) {
                (Some(symbol), _) => format!("{symbol}-2d.png"),
                (None, pcbcraft_domain::geometry::Face::Back) => "back-2d.png".into(),
                (None, pcbcraft_domain::geometry::Face::Front) => "2d.png".into(),
            };
            PathBuf::from(format!("{}-{suffix}", project.stem()))
        }),
    );
    KiCad::from_env().render_top(board, &output, args.width, face.render_side(), frame)?;
    Ok(CommandOutput::note(format!("Wrote {}\n", output.display())))
}

fn write_out(path: &Path, text: &str) -> Result<CommandOutput> {
    if path == Path::new("-") {
        return Ok(CommandOutput::text(text.to_string()));
    }
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }
    fs::write(path, text).with_context(|| format!("cannot write {}", path.display()))?;
    Ok(CommandOutput::note(format!("Wrote {}\n", path.display())))
}
