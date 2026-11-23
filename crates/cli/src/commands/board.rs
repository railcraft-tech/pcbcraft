//! `pcbcraft board setup` writes fabrication floors into the KiCad project.

use std::fs;
use std::path::Path;

use anyhow::{bail, Context, Result};
use pcbcraft_domain::fab::{self, FabProfile};
use pcbcraft_kicad::board_setup::{self, read_copper};
use pcbcraft_kicad::project::{require_file, resolve_in};
use pcbcraft_kicad::KiCadCli;

use super::args::BoardSetupArgs;
use super::output::{place, CommandOutput};

pub fn setup(cwd: &Path, args: &BoardSetupArgs) -> CommandOutput {
    match setup_result(cwd, args) {
        Ok(text) => CommandOutput::text(text),
        Err(err) => {
            let message = format!("{err:#}\n");
            let code = if is_profile_error(&err) { 2 } else { 1 };
            CommandOutput::error(code, message)
        }
    }
}

fn is_profile_error(err: &anyhow::Error) -> bool {
    let text = format!("{err:#}");
    text.contains("profile")
        || text.contains("no track limit")
        || text.contains("no mask limit")
        || text.contains("unknown profile")
}

fn setup_result(cwd: &Path, args: &BoardSetupArgs) -> Result<String> {
    let profile = load_profile(cwd, args)?;
    let project = resolve_in(cwd, args.input.as_deref(), None, None)?;
    let pro = match &project.pro {
        Some(pro) => pro.clone(),
        None => {
            let board = project.board.as_ref().context("no project file")?;
            let sibling = board.with_extension("kicad_pro");
            if sibling.is_file() {
                sibling
            } else {
                bail!("no .kicad_pro next to {}", board.display());
            }
        }
    };
    let board = project
        .board
        .clone()
        .unwrap_or_else(|| pro.with_extension("kicad_pcb"));
    require_file(&pro, "project")?;
    require_file(&board, "board")?;
    let copper = read_copper(&fs::read_to_string(&board)?)?;
    let limits = fab::limits(&profile, copper.layers, copper.outer_mm, &args.mask)?;
    let cli = KiCadCli::from_env();
    board_setup::apply(&pro, &board, &limits, &cli.bin)
}

fn load_profile(cwd: &Path, args: &BoardSetupArgs) -> Result<FabProfile> {
    match args.profile.as_str() {
        "jlcpcb" => {
            if args.file.is_some() {
                bail!("jlcpcb does not take a profile file");
            }
            Ok(fab::jlcpcb())
        }
        "pcbway" => {
            if args.file.is_some() {
                bail!("pcbway does not take a profile file");
            }
            Ok(fab::pcbway())
        }
        "custom" => {
            let file = args.file.as_ref().context("custom profile needs a file")?;
            let path = place(cwd, file);
            let text =
                fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
            fab::from_json(&text)
        }
        other => bail!("unknown profile {other}"),
    }
}
