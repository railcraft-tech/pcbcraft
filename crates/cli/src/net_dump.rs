use anyhow::Result;
use pcbcraft_application::net::render_dump;
use pcbcraft_domain::net::NetGraph;

pub fn render(graph: &NetGraph, name: &str) -> Result<String> {
    Ok(render_dump(graph, name)?)
}
