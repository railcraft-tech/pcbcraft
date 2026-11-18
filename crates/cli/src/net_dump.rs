use anyhow::Result;
use pcbcraft_application::net::render_dump;
use pcbcraft_domain::net::NetGraph;

pub fn print_net(graph: &NetGraph, name: &str) -> Result<()> {
    let text = render_dump(graph, name)?;
    print!("{text}");
    Ok(())
}
