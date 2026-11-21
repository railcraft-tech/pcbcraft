use std::path::Path;

use anyhow::Result;
use pcbcraft_kicad::net::load_graph;
use pcbcraft_kicad::project::resolve_in;
use pcbcraft_kicad::KiCad;

use super::args::NetDumpArgs;
use super::output::CommandOutput;
use crate::net_dump;

pub fn dump(cwd: &Path, args: &NetDumpArgs) -> CommandOutput {
    CommandOutput::from_result(dump_result(cwd, args))
}

fn dump_result(cwd: &Path, args: &NetDumpArgs) -> Result<CommandOutput> {
    let project = resolve_in(cwd, args.project.as_deref(), None, None)?;
    let graph = load_graph(&KiCad::from_env().cli, &project)?;
    Ok(CommandOutput::text(net_dump::render(&graph, &args.name)?))
}
