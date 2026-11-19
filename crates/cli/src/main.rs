//! pcbcraft — exports, checks, and part lookup for KiCad boards.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use clap::builder::PossibleValuesParser;
use clap::{Args, Parser, Subcommand};
use pcbcraft_application::export::{correct_position, mechanical_page, rewrite_comments};
use pcbcraft_application::parts::{search_all, sync_components, usages_from_graph};
use pcbcraft_application::ports::{
    ComponentStore, Fabrication, PartStock, RotationDb, TestPrinter,
};
use pcbcraft_application::validate::{
    known_types, order, relative_path, run_tests_skipping, write_one, write_scaffold, LoadedTest,
    TestContext,
};
use pcbcraft_domain::drawing::Side;
use pcbcraft_domain::parts::{parse_spec, Query, SupplierCatalog};
use pcbcraft_domain::validate::TestFile;
use pcbcraft_kicad::dxf::parse_outline;
use pcbcraft_kicad::excellon;
use pcbcraft_kicad::gerbers::zip_dir;
use pcbcraft_kicad::net::load_graph;
use pcbcraft_kicad::project::{require_file, resolve};
use pcbcraft_kicad::rotation_db::{read_overlay, CachedRotationDb};
use pcbcraft_kicad::{KiCad, SqliteComponents};
use pcbcraft_render::PdfRenderer;
use pcbcraft_suppliers::{LcscCatalog, PcbwayCatalog};

use crate::printer::{memory_label, stdout_printer};

mod geometry_cmd;
mod net_dump;
mod printer;
mod show_part;

const CPL_HELP: &str = "\
Corrects rotation and offset the same way as jlc-cpl, then writes the five JLCPCB columns.
Running it again on an already corrected file mirrors the bottom a second time.
";

#[derive(Parser)]
#[command(
    name = "pcbcraft",
    version,
    about = "KiCad board export, checks, and ordering"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Manufacturing export.
    Export {
        #[command(subcommand)]
        kind: ExportKind,
    },
    /// JSON tests for a board.
    Tests {
        #[command(subcommand)]
        kind: TestsKind,
    },
    /// Search supplier catalogs.
    Parts {
        #[command(subcommand)]
        kind: PartsKind,
    },
    /// Database of parts used on the board.
    Components {
        #[command(subcommand)]
        kind: ComponentsKind,
    },
    /// Net graph.
    Net(NetArgs),
    /// Board geometry.
    Geometry {
        #[command(subcommand)]
        kind: geometry_cmd::GeometryKind,
    },
}

#[derive(Subcommand)]
enum ExportKind {
    /// JLCPCB-shaped bill of materials.
    Bom(FileArgs),
    /// Raw KiCad position file.
    Pos(FileArgs),
    /// JLCPCB CPL.
    #[command(long_about = CPL_HELP)]
    Cpl(CplArgs),
    /// Gerbers and drill files in a zip.
    Gerbers(FileArgs),
    /// 2D drawing on A4.
    Drawing(DrawingArgs),
    /// Orthographic render of the 3D view.
    #[command(name = "2d")]
    TwoD(TwoDArgs),
}

#[derive(Subcommand)]
enum PartsKind {
    /// Alternatives from a YAML spec.
    Alternatives(AltArgs),
}

#[derive(Subcommand)]
enum ComponentsKind {
    /// Fetch LCSC and PCBWay cards for parts on the schematic.
    Sync(SyncArgs),
    /// Print one catalog card from the local database as JSON.
    Show(ShowArgs),
}

#[derive(Args)]
struct FileArgs {
    /// A .kicad_pro, .kicad_sch, or .kicad_pcb file.
    input: Option<PathBuf>,
    #[arg(short, long)]
    output: Option<PathBuf>,
    #[arg(long)]
    schematic: Option<PathBuf>,
    #[arg(long)]
    board: Option<PathBuf>,
}

#[derive(Args)]
struct CplArgs {
    /// A project, a board, or an existing position CSV.
    input: Option<PathBuf>,
    #[arg(short, long)]
    output: Option<PathBuf>,
    #[arg(long)]
    bom: Option<PathBuf>,
    #[arg(long)]
    local: Option<PathBuf>,
    #[arg(long)]
    db: Option<PathBuf>,
    #[arg(long = "corrections")]
    corrections: Vec<PathBuf>,
    #[arg(long)]
    update: bool,
    #[arg(long)]
    cache_dir: Option<PathBuf>,
    #[arg(long)]
    dry_run: bool,
    #[arg(short, long)]
    quiet: bool,
    #[arg(long)]
    schematic: Option<PathBuf>,
    #[arg(long)]
    board: Option<PathBuf>,
}

#[derive(Args)]
struct DrawingArgs {
    input: Option<PathBuf>,
    #[arg(short, long)]
    output: Option<PathBuf>,
    /// `side` or `back`. Without it, both faces share one page.
    #[arg(long)]
    side: Option<String>,
    /// Full color for the selected face.
    #[arg(long)]
    full: bool,
    #[arg(long)]
    board: Option<PathBuf>,
}

#[derive(Args)]
struct TwoDArgs {
    /// A .kicad_pro, .kicad_sch, or .kicad_pcb file.
    input: Option<PathBuf>,
    #[arg(long)]
    board: Option<PathBuf>,
    /// Footprint reference to frame, for example J1.
    #[arg(long)]
    symbol: Option<String>,
    /// `front` or `back`. With --symbol, the footprint chooses the face.
    #[arg(long, default_value = "front")]
    side: String,
    /// Extra space around the symbol. Defaults to 5mm. Requires --symbol.
    #[arg(long)]
    around_space: Option<String>,
    /// Square image side in pixels.
    #[arg(long, default_value_t = 1024)]
    width: u32,
    #[arg(short, long)]
    output: Option<PathBuf>,
}

#[derive(Subcommand)]
enum TestsKind {
    /// Run the JSON tests.
    Run(RunArgs),
    /// List the tests in the directory, then the types this program can run.
    List(TestsDir),
    /// Create test files.
    Init(InitArgs),
}

#[derive(Args)]
struct RunArgs {
    /// Directory of JSON files. Defaults to tests/pcbcraft.
    directory: Option<PathBuf>,
    /// Run one test file. The name has no extension and is not a subtest.
    #[arg(long)]
    name: Option<String>,
}

#[derive(Args)]
struct TestsDir {
    /// Directory of JSON files. Defaults to tests/pcbcraft.
    directory: Option<PathBuf>,
}

#[derive(Args)]
struct InitArgs {
    #[command(subcommand)]
    kind: InitKind,
}

#[derive(Subcommand)]
enum InitKind {
    /// Write erc-test, drc-test, and availability-test.
    Defaults(TestsDir),
    /// Write one test file. Requires a type and a file name.
    #[command(after_help = type_list_help())]
    Type(InitTypeArgs),
}

#[derive(Args)]
struct InitTypeArgs {
    /// Validator type.
    #[arg(value_parser = known_type_names(), hide_possible_values = true)]
    type_name: Option<String>,
    /// Test name. This is the file name without an extension.
    name: Option<String>,
    /// Directory of JSON files. Defaults to tests/pcbcraft.
    #[arg(long)]
    directory: Option<PathBuf>,
}

fn known_type_names() -> PossibleValuesParser {
    PossibleValuesParser::new(known_types().iter().copied())
}

fn type_list_help() -> &'static str {
    use std::sync::OnceLock;
    static TEXT: OnceLock<String> = OnceLock::new();
    TEXT.get_or_init(|| {
        let mut text = String::from("Known types:\n");
        for name in known_types() {
            text.push_str("  ");
            text.push_str(name);
            text.push('\n');
        }
        text
    })
    .as_str()
}

#[derive(Args)]
struct AltArgs {
    spec: PathBuf,
    #[arg(long, default_value_t = 10)]
    limit: usize,
    #[arg(long)]
    include_out_of_stock: bool,
    #[arg(long)]
    basic: bool,
    #[arg(long, value_parser = ["text", "yaml"])]
    format: Option<String>,
}

#[derive(Args)]
struct SyncArgs {
    input: Option<PathBuf>,
    #[arg(long)]
    refresh: bool,
}

#[derive(Args)]
struct ShowArgs {
    /// LCSC or PCBWay catalog number, for example C12345.
    part: String,
    /// A .kicad_pro, .kicad_sch, or .kicad_pcb file.
    input: Option<PathBuf>,
}

#[derive(Args)]
struct NetArgs {
    name: String,
    #[command(subcommand)]
    kind: NetKind,
    #[arg(long)]
    project: Option<PathBuf>,
}

#[derive(Subcommand)]
enum NetKind {
    /// Graph and pin list.
    Dump,
}

fn main() {
    if let Err(err) = run() {
        eprintln!("pcbcraft: {err:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    match Cli::parse().command {
        Command::Export { kind } => match kind {
            ExportKind::Bom(args) => export_bom(args),
            ExportKind::Pos(args) => export_pos(args),
            ExportKind::Cpl(args) => export_cpl(args),
            ExportKind::Gerbers(args) => export_gerbers(args),
            ExportKind::Drawing(args) => export_drawing(args),
            ExportKind::TwoD(args) => export_2d(args),
        },
        Command::Tests { kind } => dispatch_tests(kind),
        Command::Parts { kind } => match kind {
            PartsKind::Alternatives(args) => alternatives(args),
        },
        Command::Components { kind } => match kind {
            ComponentsKind::Sync(args) => sync(args),
            ComponentsKind::Show(args) => show(args),
        },
        Command::Net(args) => match args.kind {
            NetKind::Dump => net_dump(&args.name, args.project.as_deref()),
        },
        Command::Geometry { kind } => geometry_cmd::run(kind),
    }
}

fn export_bom(args: FileArgs) -> Result<()> {
    let project = resolve(
        args.input.as_deref(),
        args.schematic.as_deref(),
        args.board.as_deref(),
    )?;
    let schematic = project.schematic.as_ref().context("no schematic")?;
    require_file(schematic, "schematic")?;
    let output = args
        .output
        .unwrap_or_else(|| PathBuf::from(format!("{}-bom.csv", project.stem())));
    let temp = std::env::temp_dir().join(format!("pcbcraft-bom-{}.csv", std::process::id()));
    KiCad::from_env().export_bom(schematic, &temp)?;
    let text = fs::read_to_string(&temp)?;
    write_out(&output, &rewrite_comments(&text)?)?;
    Ok(())
}

fn export_pos(args: FileArgs) -> Result<()> {
    let project = resolve(
        args.input.as_deref(),
        args.schematic.as_deref(),
        args.board.as_deref(),
    )?;
    let board = project.board.as_ref().context("no board")?;
    require_file(board, "board")?;
    let output = args
        .output
        .unwrap_or_else(|| PathBuf::from(format!("{}-pos.csv", project.stem())));
    KiCad::from_env().export_pos(board, &output)?;
    eprintln!("Wrote {}", output.display());
    Ok(())
}

fn export_cpl(args: CplArgs) -> Result<()> {
    let input_is_csv = args
        .input
        .as_ref()
        .is_some_and(|path| path.extension().and_then(|ext| ext.to_str()) == Some("csv"));
    let (pos_text, bom_text, stem) = if input_is_csv {
        let path = args.input.as_ref().unwrap();
        let bom = args
            .bom
            .as_ref()
            .map(|path| fs::read_to_string(path))
            .transpose()?;
        (
            fs::read_to_string(path)?,
            bom,
            path.file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
                .unwrap_or_else(|| "positions".into()),
        )
    } else {
        let project = resolve(
            args.input.as_deref(),
            args.schematic.as_deref(),
            args.board.as_deref(),
        )?;
        let schematic = project.schematic.as_ref().context("no schematic")?;
        let board = project.board.as_ref().context("no board")?;
        require_file(schematic, "schematic")?;
        require_file(board, "board")?;
        let dir = std::env::temp_dir().join(format!("pcbcraft-cpl-{}", std::process::id()));
        fs::create_dir_all(&dir)?;
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
        cache_dir: args.cache_dir.clone(),
        explicit: args.db.clone(),
    };
    let database = db.load(args.update)?;
    let local = args
        .local
        .as_ref()
        .map(|path| fs::read_to_string(path))
        .transpose()?;
    let mut extras = Vec::new();
    for path in &args.corrections {
        extras.push(read_overlay(path)?);
    }
    let produced = correct_position(
        &pos_text,
        bom_text.as_deref().or(args.bom.as_ref().and_then(|_| None)),
        &database,
        local.as_deref(),
        &extras,
    )?;
    if !args.quiet && !produced.report.is_empty() {
        eprint!("{}", produced.report);
    }
    if args.dry_run {
        eprintln!("Not writing (dry-run).");
        return Ok(());
    }
    let output = args
        .output
        .unwrap_or_else(|| PathBuf::from(format!("{stem}-cpl.csv")));
    write_out(&output, &produced.csv)?;
    Ok(())
}

fn export_gerbers(args: FileArgs) -> Result<()> {
    let project = resolve(
        args.input.as_deref(),
        args.schematic.as_deref(),
        args.board.as_deref(),
    )?;
    let board = project.board.as_ref().context("no board")?;
    require_file(board, "board")?;
    let dir = std::env::temp_dir().join(format!("pcbcraft-gerber-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir)?;
    let kicad = KiCad::from_env();
    kicad.export_gerbers(board, &dir)?;
    kicad.export_drill_split(board, &dir)?;
    let output = args
        .output
        .unwrap_or_else(|| PathBuf::from(format!("{}-gerbers.zip", project.stem())));
    zip_dir(&dir, &output)?;
    eprintln!("Wrote {}", output.display());
    let _ = fs::remove_dir_all(&dir);
    Ok(())
}

fn export_drawing(args: DrawingArgs) -> Result<()> {
    let project = resolve(args.input.as_deref(), None, args.board.as_deref())?;
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
    let dir = std::env::temp_dir().join(format!("pcbcraft-draw-{}", std::process::id()));
    fs::create_dir_all(&dir)?;
    let output = args
        .output
        .unwrap_or_else(|| PathBuf::from(format!("{}-drawing.pdf", project.stem())));
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
    eprintln!("Wrote {}", output.display());
    Ok(())
}

fn export_2d(args: TwoDArgs) -> Result<()> {
    if args.around_space.is_some() && args.symbol.is_none() {
        bail!("--around-space requires --symbol");
    }
    let project = resolve(args.input.as_deref(), None, args.board.as_deref())?;
    let board = project.board.as_ref().context("no board")?;
    require_file(board, "board")?;
    let requested = pcbcraft_domain::geometry::Face::parse(&args.side)
        .with_context(|| format!("--side must be front or back, not '{}'", args.side))?;
    let (frame, face) = if let Some(symbol) = &args.symbol {
        let margin = match &args.around_space {
            Some(text) => pcbcraft_domain::geometry::parse_millimetres(text)?,
            None => 5.0,
        };
        let geometry = geometry_cmd::load_geometry(args.input.as_deref(), args.board.as_deref())?;
        let (bounds, shapes) = pcbcraft_application::geometry::symbol_view(&geometry, symbol)?;
        let square = pcbcraft_domain::geometry::square_around(bounds, margin);
        let board_window = pcbcraft_domain::geometry::board_window(&geometry);
        let frame = pcbcraft_domain::geometry::render_frame(board_window, square);
        let face = pcbcraft_domain::geometry::symbol_face(&shapes);
        (Some(frame), face)
    } else {
        (None, requested)
    };
    let output = args.output.unwrap_or_else(|| {
        let suffix = match (&args.symbol, face) {
            (Some(symbol), _) => format!("{symbol}-2d.png"),
            (None, pcbcraft_domain::geometry::Face::Back) => "back-2d.png".into(),
            (None, pcbcraft_domain::geometry::Face::Front) => "2d.png".into(),
        };
        PathBuf::from(format!("{}-{suffix}", project.stem()))
    });
    KiCad::from_env().render_top(board, &output, args.width, face.render_side(), frame)?;
    eprintln!("Wrote {}", output.display());
    Ok(())
}

fn dispatch_tests(kind: TestsKind) -> Result<()> {
    let code = match kind {
        TestsKind::Run(args) => run_test(
            &test_dir_opt(args.directory.as_deref()),
            args.name.as_deref(),
        )?,
        TestsKind::List(args) => list_tests(&test_dir(&args))?,
        TestsKind::Init(args) => match args.kind {
            InitKind::Defaults(dir) => init_tests(&test_dir(&dir))?,
            InitKind::Type(typed) => init_typed(
                &test_dir_opt(typed.directory.as_deref()),
                typed.type_name.as_deref(),
                typed.name.as_deref(),
            )?,
        },
    };
    if code != 0 {
        std::process::exit(code);
    }
    Ok(())
}

fn test_dir(args: &TestsDir) -> PathBuf {
    test_dir_opt(args.directory.as_deref())
}

fn test_dir_opt(directory: Option<&Path>) -> PathBuf {
    directory
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("tests/pcbcraft"))
}

fn run_test(directory: &Path, name: Option<&str>) -> Result<i32> {
    if !directory.is_dir() {
        eprintln!("no directory {}", directory.display());
        return Ok(2);
    }
    let mut files = Vec::new();
    collect_json(directory, directory, &mut files)?;
    if let Some(name) = name {
        let wanted = keep_named(&mut files, name);
        if files.is_empty() {
            eprintln!("no test '{wanted}'");
            return Ok(2);
        }
    } else if files.is_empty() {
        eprintln!("no JSON files in {}", directory.display());
        return Ok(2);
    }
    let mut tests = Vec::new();
    let mut parse_errors = Vec::new();
    for (name, path) in files {
        let text = fs::read_to_string(&path)?;
        match serde_json::from_str::<TestFile>(&text) {
            Ok(file) => {
                let mut spec = file.spec;
                absolutize(&path, &mut spec);
                tests.push(LoadedTest {
                    name,
                    type_name: file.type_name,
                    spec,
                });
            }
            Err(err) => parse_errors.push(format!("{name}: {err}")),
        }
    }
    let mut printer = stdout_printer();
    if !parse_errors.is_empty() {
        printer.schema_errors(&parse_errors);
        return Ok(2);
    }
    let kicad = KiCad::from_env();
    let stock = LiveStock {
        lcsc: LcscCatalog::default(),
        pcbway: PcbwayCatalog,
    };
    let cards = pcbcraft_kicad::ProjectCards;
    let ctx = TestContext {
        fabrication: &kicad,
        used_parts: &kicad,
        stock: &stock,
        graph: &kicad,
        cards: &cards,
        boards: &kicad,
    };
    let skipped = match load_skips() {
        Ok(skipped) => skipped,
        Err(err) => {
            eprintln!("pcbcraft: {err:#}");
            return Ok(2);
        }
    };
    let totals = run_tests_skipping(tests, &ctx, &mut printer, &skipped)?;
    if totals.blocked {
        return Ok(2);
    }
    printer.summary(
        totals.elapsed,
        &memory_label(),
        totals.failed == 0,
        totals.tests,
        totals.assertions,
        totals.skipped,
        totals.warnings,
    );
    for entry in totals.unmatched_skips {
        eprintln!("skipped entry '{entry}' did not match a test");
    }
    Ok(if totals.failed == 0 { 0 } else { 1 })
}

fn load_skips() -> Result<Vec<String>> {
    let path = Path::new("pcbcraft.yaml");
    if !path.is_file() {
        return Ok(Vec::new());
    }
    let text =
        fs::read_to_string(path).with_context(|| format!("cannot read {}", path.display()))?;
    let config =
        pcbcraft_domain::config::parse_config(&text).map_err(|err| anyhow::anyhow!(err))?;
    Ok(config.skipped)
}

struct LiveStock {
    lcsc: LcscCatalog,
    pcbway: PcbwayCatalog,
}

impl PartStock for LiveStock {
    fn lookup(
        &self,
        supplier: pcbcraft_domain::parts::Supplier,
        part_id: &str,
    ) -> Result<Option<u64>> {
        let records = match supplier {
            pcbcraft_domain::parts::Supplier::Lcsc => self.lcsc.fetch(&[part_id.to_string()])?,
            pcbcraft_domain::parts::Supplier::Pcbway => {
                self.pcbway.fetch(&[part_id.to_string()])?
            }
        };
        Ok(records.into_iter().next().and_then(|record| record.stock))
    }
}

fn list_tests(directory: &Path) -> Result<i32> {
    let mut loaded = Vec::new();
    let mut broken = Vec::new();
    if directory.is_dir() {
        let mut files = Vec::new();
        collect_json(directory, directory, &mut files)?;
        for (name, path) in files {
            let text = fs::read_to_string(&path)?;
            match serde_json::from_str::<TestFile>(&text) {
                Ok(file) => loaded.push(LoadedTest {
                    name,
                    type_name: file.type_name,
                    spec: file.spec,
                }),
                Err(err) => broken.push((name, err.to_string())),
            }
        }
    }
    broken.sort_by(|left, right| left.0.cmp(&right.0));
    print!(
        "{}",
        format_test_list(&order(loaded), &broken, known_types())
    );
    Ok(0)
}

fn format_test_list(tests: &[LoadedTest], broken: &[(String, String)], types: &[&str]) -> String {
    let mut out = String::new();
    if tests.is_empty() && broken.is_empty() {
        out.push_str("(no tests)\n");
    }
    for test in tests {
        out.push_str(&format!("{}  {}\n", test.name, test.type_name));
    }
    for (name, err) in broken {
        out.push_str(&format!("{name}  invalid JSON: {err}\n"));
    }
    out.push('\n');
    out.push_str("Types\n");
    for type_name in types {
        out.push_str(&format!("{type_name}\n"));
    }
    out
}

fn init_tests(directory: &Path) -> Result<i32> {
    if directory.is_file() {
        bail!("{} is a file, not a directory", directory.display());
    }
    let project = discover_project()?;
    let project_path = project
        .clone()
        .unwrap_or_else(|| PathBuf::from("project.kicad_pro"));
    let relative = relative_path(directory, &project_path);
    let created = write_scaffold(directory, &relative)?;
    if created.is_empty() {
        eprintln!("{} already has the default tests", directory.display());
    } else {
        for path in &created {
            eprintln!("Created {}", path.display());
        }
        if project.is_none() {
            eprintln!("Set spec.project in those files to your .kicad_pro.");
        }
    }
    Ok(0)
}

fn init_typed(directory: &Path, type_name: Option<&str>, name: Option<&str>) -> Result<i32> {
    let (Some(type_name), Some(name)) = (type_name, name) else {
        eprint!("{}", missing_type_choice());
        return Ok(2);
    };
    if directory.is_file() {
        bail!("{} is a file, not a directory", directory.display());
    }
    if !known_types().contains(&type_name) {
        bail!("unknown test type '{type_name}'");
    }
    let project = discover_project()?;
    let project_path = project
        .clone()
        .unwrap_or_else(|| PathBuf::from("project.kicad_pro"));
    let relative = relative_path(directory, &project_path);
    match write_one(directory, type_name, name, &relative)? {
        Some(path) => {
            eprintln!("Created {}", path.display());
            if project.is_none() {
                eprintln!("Set spec.project in those files to your .kicad_pro.");
            }
        }
        None => eprintln!(
            "{} already exists",
            directory
                .join(if name.ends_with(".json") {
                    name.to_string()
                } else {
                    format!("{name}.json")
                })
                .display()
        ),
    }
    Ok(0)
}

fn missing_type_choice() -> String {
    format!(
        "Choose a test type and a test name (file name without an extension).\n\n{}",
        type_list_help()
    )
}

fn discover_project() -> Result<Option<PathBuf>> {
    let cwd = std::env::current_dir().context("current directory")?;
    let mut found = Vec::new();
    for entry in fs::read_dir(&cwd).with_context(|| format!("{}", cwd.display()))? {
        let entry = entry?;
        if entry.path().extension().and_then(|ext| ext.to_str()) == Some("kicad_pro") {
            found.push(entry.path());
        }
    }
    match found.len() {
        0 => Ok(None),
        1 => Ok(Some(found.remove(0))),
        _ => Ok(None),
    }
}

fn absolutize(json_path: &Path, spec: &mut serde_json::Value) {
    let Some(project) = spec.get("project").and_then(|value| value.as_str()) else {
        return;
    };
    let path = Path::new(project);
    if path.is_absolute() {
        return;
    }
    if let Some(parent) = json_path.parent() {
        let absolute = parent.join(path);
        spec["project"] = serde_json::Value::String(absolute.display().to_string());
    }
}

fn keep_named(files: &mut Vec<(String, PathBuf)>, name: &str) -> String {
    let wanted = name.strip_suffix(".json").unwrap_or(name).to_string();
    files.retain(|(test_name, _)| test_name == &wanted);
    wanted
}

fn collect_json(root: &Path, dir: &Path, out: &mut Vec<(String, PathBuf)>) -> Result<()> {
    let mut entries = fs::read_dir(dir)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            collect_json(root, &path, out)?;
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("json") {
            let name = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .with_extension("")
                .to_string_lossy()
                .replace('\\', "/");
            out.push((name, path));
        }
    }
    Ok(())
}

fn alternatives(args: AltArgs) -> Result<()> {
    let text = fs::read_to_string(&args.spec)?;
    let spec = parse_spec(&text)?;
    let lcsc = LcscCatalog {
        in_stock: !args.include_out_of_stock,
        basic_only: args.basic,
    };
    let found = search_all(&spec.items, &lcsc, &PcbwayCatalog, args.limit)?;
    if args.format.as_deref() == Some("yaml") {
        println!("{}", to_yaml(&found)?);
    } else {
        for item in &found {
            println!("{}", item.name);
            if item.hits.is_empty() {
                println!("  (none)");
                continue;
            }
            for hit in &item.hits {
                println!(
                    "  {} {} {} {} stock={}",
                    hit.supplier,
                    hit.part_id,
                    hit.mpn,
                    hit.package,
                    hit.stock
                        .map(|n| n.to_string())
                        .unwrap_or_else(|| "-".into())
                );
            }
        }
    }
    let _ = Query::Lcsc;
    Ok(())
}

fn to_yaml(found: &[pcbcraft_application::parts::Found]) -> Result<String> {
    let mut out = String::from("items:\n");
    for item in found {
        out.push_str(&format!("  - name: {}\n    matches:\n", item.name));
        for hit in &item.hits {
            out.push_str(&format!(
                "      - supplier: {}\n        id: {}\n        mpn: \"{}\"\n        package: \"{}\"\n",
                hit.supplier, hit.part_id, hit.mpn, hit.package
            ));
        }
    }
    Ok(out)
}

fn sync(args: SyncArgs) -> Result<()> {
    let project = resolve(args.input.as_deref(), None, None)?;
    let graph = load_graph(&KiCad::from_env().cli, &project)?;
    let usage = usages_from_graph(&graph);
    let store = SqliteComponents::new(project.component_db());
    let stats = sync_components(
        &usage,
        &store,
        &LcscCatalog::default(),
        &PcbwayCatalog,
        args.refresh,
    )?;
    println!(
        "Wrote {} (fetched {}, kept {}, used {})",
        project.component_db().display(),
        stats.fetched,
        stats.kept,
        stats.used
    );
    Ok(())
}

fn show(args: ShowArgs) -> Result<()> {
    let project = resolve(args.input.as_deref(), None, None)?;
    let store = SqliteComponents::new(project.component_db());
    let found = store.find(&args.part)?;
    if found.is_empty() {
        bail!(
            "no part '{}' in {}",
            args.part,
            project.component_db().display()
        );
    }
    print!("{}", show_part::render(&found)?);
    Ok(())
}

fn net_dump(name: &str, project: Option<&Path>) -> Result<()> {
    let project = resolve(project, None, None)?;
    let graph = load_graph(&KiCad::from_env().cli, &project)?;
    crate::net_dump::print_net(&graph, name)?;
    Ok(())
}

fn write_out(path: &Path, text: &str) -> Result<()> {
    if path == Path::new("-") {
        print!("{text}");
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }
    fs::write(path, text).with_context(|| format!("cannot write {}", path.display()))?;
    eprintln!("Wrote {}", path.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::format_test_list;
    use clap::CommandFactory;
    use pcbcraft_application::validate::{known_types, order, LoadedTest};
    use std::path::PathBuf;

    #[test]
    fn the_list_puts_project_tests_above_the_known_types() {
        let tests = order(vec![
            LoadedTest {
                name: "drc-test".into(),
                type_name: "DRCValidator".into(),
                spec: serde_json::json!({}),
            },
            LoadedTest {
                name: "erc-test".into(),
                type_name: "ERCValidator".into(),
                spec: serde_json::json!({}),
            },
        ]);
        let text = format_test_list(&tests, &[], &["ERCValidator", "DRCValidator"]);
        assert_eq!(
            text,
            "\
erc-test  ERCValidator
drc-test  DRCValidator

Types
ERCValidator
DRCValidator
"
        );
    }

    #[test]
    fn init_type_help_lists_every_known_type() {
        let mut command = super::Cli::command();
        let tests = command.find_subcommand_mut("tests").unwrap();
        let init = tests.find_subcommand_mut("init").unwrap();
        let typed = init.find_subcommand_mut("type").unwrap();
        let mut buffer = Vec::new();
        typed.write_long_help(&mut buffer).unwrap();
        let text = String::from_utf8(buffer).unwrap();
        assert!(text.contains("Known types:"), "{text}");
        for name in known_types() {
            assert!(text.contains(name), "{name} missing from:\n{text}");
        }
    }

    #[test]
    fn a_missing_type_asks_for_the_type_and_the_file_name() {
        let text = super::missing_type_choice();
        assert!(text.contains("Choose a test type and a test name"));
        assert!(text.contains("without an extension"));
        for name in known_types() {
            assert!(text.contains(name), "{name}");
        }
    }

    #[test]
    fn name_selects_one_file_and_not_a_prefix() {
        let mut files = vec![
            ("erc-test".into(), PathBuf::from("erc-test.json")),
            (
                "erc-test-extra".into(),
                PathBuf::from("erc-test-extra.json"),
            ),
            (
                "checks/erc-test".into(),
                PathBuf::from("checks/erc-test.json"),
            ),
        ];
        let wanted = super::keep_named(&mut files, "erc-test.json");
        assert_eq!(wanted, "erc-test");
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].0, "erc-test");
    }
}
