//! Argument structs shared by the CLI and the MCP tools.
//! An omitted field means the same thing as an omitted flag.

use std::path::PathBuf;

use clap::builder::PossibleValuesParser;
use clap::Args;
use pcbcraft_application::validate::known_types;
use serde::Deserialize;

fn default_side() -> String {
    "front".into()
}

fn default_width() -> u32 {
    1024
}

fn default_limit() -> usize {
    10
}

#[derive(Debug, Args, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct FileArgs {
    /// A .kicad_pro, .kicad_sch, or .kicad_pcb file.
    pub input: Option<PathBuf>,
    #[arg(short, long)]
    pub output: Option<PathBuf>,
    #[arg(long)]
    pub schematic: Option<PathBuf>,
    #[arg(long)]
    pub board: Option<PathBuf>,
}

#[derive(Debug, Args, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct CplArgs {
    /// A project, a board, or an existing position CSV.
    pub input: Option<PathBuf>,
    #[arg(short, long)]
    pub output: Option<PathBuf>,
    #[arg(long)]
    pub bom: Option<PathBuf>,
    #[arg(long)]
    pub local: Option<PathBuf>,
    #[arg(long)]
    pub db: Option<PathBuf>,
    #[arg(long = "corrections")]
    pub corrections: Vec<PathBuf>,
    #[arg(long)]
    pub update: bool,
    #[arg(long)]
    pub cache_dir: Option<PathBuf>,
    #[arg(long)]
    pub dry_run: bool,
    #[arg(short, long)]
    pub quiet: bool,
    #[arg(long)]
    pub schematic: Option<PathBuf>,
    #[arg(long)]
    pub board: Option<PathBuf>,
}

#[derive(Debug, Args, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct DrawingArgs {
    pub input: Option<PathBuf>,
    #[arg(short, long)]
    pub output: Option<PathBuf>,
    /// `side` or `back`. Without it, both faces share one page.
    #[arg(long)]
    pub side: Option<String>,
    /// Full color for the selected face.
    #[arg(long)]
    pub full: bool,
    #[arg(long)]
    pub board: Option<PathBuf>,
}

#[derive(Debug, Args, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TwoDArgs {
    /// A .kicad_pro, .kicad_sch, or .kicad_pcb file.
    #[serde(default)]
    pub input: Option<PathBuf>,
    #[arg(long)]
    #[serde(default)]
    pub board: Option<PathBuf>,
    /// Footprint reference to frame, for example J1.
    #[arg(long)]
    #[serde(default)]
    pub symbol: Option<String>,
    /// `front` or `back`. With --symbol, the footprint chooses the face.
    #[arg(long, default_value = "front")]
    #[serde(default = "default_side")]
    pub side: String,
    /// Extra space around the symbol. Defaults to 5mm. Requires --symbol.
    #[arg(long)]
    #[serde(default)]
    pub around_space: Option<String>,
    /// Square image side in pixels.
    #[arg(long, default_value_t = 1024)]
    #[serde(default = "default_width")]
    pub width: u32,
    #[arg(short, long)]
    #[serde(default)]
    pub output: Option<PathBuf>,
}

impl Default for TwoDArgs {
    fn default() -> Self {
        Self {
            input: None,
            board: None,
            symbol: None,
            side: default_side(),
            around_space: None,
            width: default_width(),
            output: None,
        }
    }
}

#[derive(Debug, Args, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct RunArgs {
    /// Directory of JSON files. Defaults to tests/pcbcraft.
    pub directory: Option<PathBuf>,
    /// Run one test file. The name has no extension and is not a subtest.
    #[arg(long)]
    pub name: Option<String>,
}

#[derive(Debug, Args, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct TestsDir {
    /// Directory of JSON files. Defaults to tests/pcbcraft.
    pub directory: Option<PathBuf>,
}

#[derive(Debug, Args, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct InitTypeArgs {
    /// Validator type.
    #[arg(value_parser = known_type_names(), hide_possible_values = true)]
    pub type_name: Option<String>,
    /// Test name. This is the file name without an extension.
    pub name: Option<String>,
    /// Directory of JSON files. Defaults to tests/pcbcraft.
    #[arg(long)]
    pub directory: Option<PathBuf>,
}

pub fn known_type_names() -> PossibleValuesParser {
    PossibleValuesParser::new(known_types().iter().copied())
}

pub fn type_list_help() -> &'static str {
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

#[derive(Debug, Args, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AltArgs {
    pub spec: PathBuf,
    #[arg(long, default_value_t = 10)]
    #[serde(default = "default_limit")]
    pub limit: usize,
    #[arg(long)]
    #[serde(default)]
    pub include_out_of_stock: bool,
    #[arg(long)]
    #[serde(default)]
    pub basic: bool,
    #[arg(long, value_parser = ["text", "yaml"])]
    #[serde(default)]
    pub format: Option<String>,
}

#[derive(Debug, Args, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct SyncArgs {
    pub input: Option<PathBuf>,
    #[arg(long)]
    pub refresh: bool,
}

#[derive(Debug, Args, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShowArgs {
    /// LCSC or PCBWay catalog number, for example C12345.
    pub part: String,
    /// A .kicad_pro, .kicad_sch, or .kicad_pcb file.
    #[serde(default)]
    pub input: Option<PathBuf>,
}

#[derive(Debug, Args, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NetDumpArgs {
    pub name: String,
    #[arg(long)]
    #[serde(default)]
    pub project: Option<PathBuf>,
}
