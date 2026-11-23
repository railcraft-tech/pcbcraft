//! pcbcraft — exports, checks, and part lookup for KiCad boards.

use std::io::{self, IsTerminal, Read, Write};

use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand};

use crate::commands::args::{
    type_list_help, AltArgs, CplArgs, DatasheetArgs, DownloadArgs, DrawingArgs, FileArgs,
    InitTypeArgs, NetDumpArgs, RunArgs, SearchArgs, ShowArgs, SingleRunArgs, SyncArgs, TestsDir,
    TwoDArgs,
};
use crate::commands::output::CommandOutput;

mod commands;
mod geometry_cmd;
mod net_dump;
mod pdf2md;
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
    /// KiCad board settings.
    Board {
        #[command(subcommand)]
        kind: BoardKind,
    },
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
    /// Serve the same commands over MCP on standard input and output.
    Mcp,
}

#[derive(Subcommand)]
enum BoardKind {
    /// Apply a fabrication profile so KiCad DRC uses the fab floors.
    Setup {
        #[command(subcommand)]
        profile: SetupProfile,
    },
}

#[derive(Subcommand)]
enum SetupProfile {
    /// JLCPCB capability floors.
    Jlcpcb(SetupTarget),
    /// PCBWay standard-review floors.
    Pcbway(SetupTarget),
    /// A JSON file with the same fields as the built-in profiles.
    Custom(CustomSetup),
}

#[derive(Args)]
struct SetupTarget {
    /// A .kicad_pro, .kicad_sch, or .kicad_pcb file.
    input: Option<std::path::PathBuf>,
    /// Solder-mask color. Defaults to green.
    #[arg(long, default_value = "green")]
    mask: String,
}

#[derive(Args)]
struct CustomSetup {
    /// Path to the JSON profile.
    file: std::path::PathBuf,
    /// A .kicad_pro, .kicad_sch, or .kicad_pcb file.
    input: Option<std::path::PathBuf>,
    /// Solder-mask color. Defaults to green.
    #[arg(long, default_value = "green")]
    mask: String,
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
    /// Search the JLCPCB assembly catalog.
    Search(SearchArgs),
    /// Download an EasyEDA footprint and 3D model.
    DownloadModel(DownloadArgs),
    /// Download an LCSC datasheet as markdown.
    DownloadDatasheet(DatasheetArgs),
}

#[derive(Subcommand)]
enum ComponentsKind {
    /// Fetch LCSC and PCBWay cards for parts on the schematic.
    Sync(SyncArgs),
    /// Print one catalog card from the local database as JSON.
    Show(ShowArgs),
}

#[derive(Subcommand)]
enum TestsKind {
    /// Run the JSON tests.
    Run(RunArgs),
    /// List the tests in the directory, then the types this program can run.
    List(TestsDir),
    /// Run one test type from a spec on stdin. Does not write a file.
    #[command(after_help = type_list_help())]
    SingleRun(SingleRunArgs),
    /// Create test files.
    Init(InitArgs),
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
struct NetArgs {
    #[command(flatten)]
    dump: NetDumpArgs,
    #[command(subcommand)]
    kind: NetKind,
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
    let cwd = std::env::current_dir().context("current directory")?;
    let color = io::stdout().is_terminal();
    match Cli::parse().command {
        Command::Board { kind } => match kind {
            BoardKind::Setup { profile } => {
                let args = match profile {
                    SetupProfile::Jlcpcb(target) => commands::args::BoardSetupArgs {
                        profile: "jlcpcb".into(),
                        file: None,
                        input: target.input,
                        mask: target.mask,
                    },
                    SetupProfile::Pcbway(target) => commands::args::BoardSetupArgs {
                        profile: "pcbway".into(),
                        file: None,
                        input: target.input,
                        mask: target.mask,
                    },
                    SetupProfile::Custom(target) => commands::args::BoardSetupArgs {
                        profile: "custom".into(),
                        file: Some(target.file),
                        input: target.input,
                        mask: target.mask,
                    },
                };
                emit(commands::board::setup(&cwd, &args))
            }
        },
        Command::Export { kind } => match kind {
            ExportKind::Bom(args) => emit(commands::export::bom(&cwd, &args)),
            ExportKind::Pos(args) => emit(commands::export::pos(&cwd, &args)),
            ExportKind::Cpl(args) => emit(commands::export::cpl(&cwd, &args)),
            ExportKind::Gerbers(args) => emit(commands::export::gerbers(&cwd, &args)),
            ExportKind::Drawing(args) => emit(commands::export::drawing(&cwd, &args)),
            ExportKind::TwoD(args) => emit(commands::export::two_d(&cwd, &args)),
        },
        Command::Tests { kind } => match kind {
            TestsKind::Run(args) => emit(commands::tests_cmd::run(&cwd, &args, color)),
            TestsKind::List(args) => emit(commands::tests_cmd::list(&cwd, &args)),
            TestsKind::SingleRun(mut args) => {
                let piped = if io::stdin().is_terminal() {
                    None
                } else {
                    let mut text = String::new();
                    io::stdin().read_to_string(&mut text)?;
                    Some(text)
                };
                commands::tests_cmd::read_piped_spec(&mut args, piped)?;
                emit(commands::tests_cmd::single_run(&cwd, &args, color))
            }
            TestsKind::Init(args) => match args.kind {
                InitKind::Defaults(args) => emit(commands::tests_cmd::init_defaults(&cwd, &args)),
                InitKind::Type(args) => emit(commands::tests_cmd::init_type(&cwd, &args)),
            },
        },
        Command::Parts { kind } => match kind {
            PartsKind::Alternatives(args) => emit(commands::parts::alternatives(&cwd, &args)),
            PartsKind::Search(mut args) => {
                let piped = if io::stdin().is_terminal() {
                    None
                } else {
                    let mut text = String::new();
                    io::stdin().read_to_string(&mut text)?;
                    Some(text)
                };
                commands::parts::read_piped_attributes(&mut args, piped)?;
                emit(commands::parts::search(&cwd, &args))
            }
            PartsKind::DownloadModel(args) => emit(commands::parts::download(&cwd, &args, color)),
            PartsKind::DownloadDatasheet(args) => {
                emit(commands::parts::download_datasheet(&cwd, &args))
            }
        },
        Command::Components { kind } => match kind {
            ComponentsKind::Sync(args) => emit(commands::components::sync(&cwd, &args)),
            ComponentsKind::Show(args) => emit(commands::components::show(&cwd, &args)),
        },
        Command::Net(args) => match args.kind {
            NetKind::Dump => emit(commands::net::dump(&cwd, &args.dump)),
        },
        Command::Geometry { kind } => emit(geometry_cmd::run(&cwd, kind)),
        Command::Mcp => commands::mcp::serve(),
    }
}

fn emit(output: CommandOutput) -> Result<()> {
    if !output.stdout.is_empty() {
        let mut stdout = io::stdout();
        write!(stdout, "{}", output.stdout)?;
        stdout.flush()?;
    }
    if !output.stderr.is_empty() {
        write!(io::stderr(), "{}", output.stderr)?;
    }
    if output.code != 0 {
        std::process::exit(output.code);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use clap::{CommandFactory, Parser};
    use pcbcraft_application::validate::known_types;

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
    fn single_run_help_lists_every_known_type() {
        let mut command = super::Cli::command();
        let tests = command.find_subcommand_mut("tests").unwrap();
        let single = tests.find_subcommand_mut("single-run").unwrap();
        let mut buffer = Vec::new();
        single.write_long_help(&mut buffer).unwrap();
        let text = String::from_utf8(buffer).unwrap();
        assert!(text.contains("Known types:"), "{text}");
        for name in known_types() {
            assert!(text.contains(name), "{name} missing from:\n{text}");
        }
    }

    #[test]
    fn single_run_parses_a_known_type_and_rejects_an_unknown_one() {
        let cli = super::Cli::try_parse_from(["pcbcraft", "tests", "single-run", "ERCValidator"])
            .unwrap();
        match cli.command {
            super::Command::Tests {
                kind: super::TestsKind::SingleRun(args),
            } => {
                assert_eq!(args.type_name, "ERCValidator");
                assert!(args.spec.is_none());
            }
            _ => panic!("expected single-run"),
        }
        let err = match super::Cli::try_parse_from(["pcbcraft", "tests", "single-run", "NotAType"])
        {
            Err(err) => err,
            Ok(_) => panic!("unknown type was accepted"),
        };
        let text = err.to_string();
        assert!(text.contains("NotAType"), "{text}");
    }

    #[test]
    fn omitted_render_fields_match_the_flag_defaults() {
        let args: crate::commands::args::TwoDArgs = serde_json::from_str("{}").unwrap();
        assert_eq!(args.side, "front");
        assert_eq!(args.width, 1024);
        assert!(args.symbol.is_none());
    }

    #[test]
    fn parts_search_reads_a_query_params_and_attribute_flags() {
        use std::path::PathBuf;

        let cli = super::Cli::try_parse_from([
            "pcbcraft",
            "parts",
            "search",
            "--supplier",
            "jlcpcb",
            "--query",
            "resistor",
            "-p",
            "package=0805",
            "-p",
            "Resistance=10kΩ",
            "--limit",
            "2",
            "params.json",
        ])
        .unwrap();
        match cli.command {
            super::Command::Parts {
                kind: super::PartsKind::Search(args),
            } => {
                assert_eq!(args.supplier, "jlcpcb");
                assert_eq!(args.query, "resistor");
                assert_eq!(args.param, vec!["package=0805", "Resistance=10kΩ"]);
                assert_eq!(args.limit, 2);
                assert_eq!(args.params, Some(PathBuf::from("params.json")));
            }
            _ => panic!("expected parts search"),
        }
    }

    #[test]
    fn parts_download_datasheet_reads_the_supplier_part_and_directory() {
        use std::path::PathBuf;

        let cli = super::Cli::try_parse_from([
            "pcbcraft",
            "parts",
            "download-datasheet",
            "--supplier",
            "lcsc",
            "C55266",
            "--output-dir",
            "context/datasheets/C55266-TPS2553DBVR",
        ])
        .unwrap();
        match cli.command {
            super::Command::Parts {
                kind: super::PartsKind::DownloadDatasheet(args),
            } => {
                assert_eq!(args.supplier, "lcsc");
                assert_eq!(args.part, "C55266");
                assert_eq!(
                    args.output_dir,
                    PathBuf::from("context/datasheets/C55266-TPS2553DBVR")
                );
            }
            _ => panic!("expected parts download-datasheet"),
        }
    }
}
