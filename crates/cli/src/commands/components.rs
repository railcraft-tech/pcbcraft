use std::path::Path;

use anyhow::{bail, Result};
use pcbcraft_application::parts::{sync_components, usages_from_graph};
use pcbcraft_application::ports::ComponentStore;
use pcbcraft_kicad::net::load_graph;
use pcbcraft_kicad::project::resolve_in;
use pcbcraft_kicad::{KiCad, SqliteComponents};
use pcbcraft_suppliers::{LcscCatalog, PcbwayCatalog};

use super::args::{ShowArgs, SyncArgs};
use super::output::CommandOutput;
use crate::show_part;

pub fn sync(cwd: &Path, args: &SyncArgs) -> CommandOutput {
    CommandOutput::from_result(sync_result(cwd, args))
}

pub fn show(cwd: &Path, args: &ShowArgs) -> CommandOutput {
    CommandOutput::from_result(show_result(cwd, args))
}

fn sync_result(cwd: &Path, args: &SyncArgs) -> Result<CommandOutput> {
    let project = resolve_in(cwd, args.input.as_deref(), None, None)?;
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
    Ok(CommandOutput::text(format!(
        "Wrote {} (fetched {}, kept {}, used {})\n",
        project.component_db().display(),
        stats.fetched,
        stats.kept,
        stats.used
    )))
}

fn show_result(cwd: &Path, args: &ShowArgs) -> Result<CommandOutput> {
    let project = resolve_in(cwd, args.input.as_deref(), None, None)?;
    let store = SqliteComponents::new(project.component_db());
    let found = store.find(&args.part)?;
    if found.is_empty() {
        bail!(
            "no part '{}' in {}",
            args.part,
            project.component_db().display()
        );
    }
    Ok(CommandOutput::text(show_part::render(&found)?))
}
