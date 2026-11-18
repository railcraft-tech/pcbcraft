use std::fmt::Write as _;

use pcbcraft_domain::net::{CopperNet, NetGraph};
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
#[error("no net '{0}'")]
pub struct DumpError(pub String);

pub fn render_dump(graph: &NetGraph, name: &str) -> Result<String, DumpError> {
    let nets: Vec<&CopperNet> = graph.named(name).collect();
    if nets.is_empty() {
        return Err(DumpError(name.to_string()));
    }
    let mut out = String::new();
    for (index, net) in nets.iter().enumerate() {
        if index > 0 {
            out.push('\n');
        }
        let _ = writeln!(
            out,
            "{}  tracks: {}",
            net.name,
            widths(&net.track_widths_mm)
        );
        out.push('\n');
        out.push_str(&graph_text(net));
        out.push('\n');
        if net.connections.is_empty() {
            out.push_str("(no pin-to-pin connections)\n");
        } else {
            for connection in &net.connections {
                let _ = writeln!(
                    out,
                    "{}.{} -> {}.{}",
                    connection.from.reference,
                    connection.from.label(),
                    connection.to.reference,
                    connection.to.label()
                );
            }
        }
    }
    Ok(out)
}

fn widths(values: &[f64]) -> String {
    if values.is_empty() {
        return "none".into();
    }
    values
        .iter()
        .map(|value| format!("{} mm", trim(*value)))
        .collect::<Vec<_>>()
        .join(", ")
}

fn trim(value: f64) -> String {
    let text = format!("{value:.4}");
    text.trim_end_matches('0').trim_end_matches('.').to_string()
}

fn graph_text(net: &CopperNet) -> String {
    let Some(first) = net.connections.first() else {
        return "(no components)\n".into();
    };
    let hub = &first.from.reference;
    let mut arms: Vec<&str> = Vec::new();
    for connection in &net.connections {
        if !arms.contains(&connection.to.reference.as_str()) {
            arms.push(connection.to.reference.as_str());
        }
    }
    let mut out = String::new();
    for (index, arm) in arms.iter().enumerate() {
        if index == 0 {
            let _ = writeln!(out, "{hub} ---- {arm}");
        } else {
            let pad = " ".repeat(hub.chars().count());
            let _ = writeln!(out, "{pad} \\--- {arm}");
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use pcbcraft_domain::net::{copper_net, NetGraph, Pin};

    fn pin(reference: &str, number: &str, name: &str) -> Pin {
        Pin {
            reference: reference.into(),
            number: number.into(),
            name: name.into(),
        }
    }

    #[test]
    fn dump_lists_the_star_under_the_graph() {
        let graph = NetGraph {
            components: Vec::new(),
            nets: vec![copper_net(
                "GND",
                vec![
                    pin("U1", "3", "GND"),
                    pin("R1", "2", ""),
                    pin("C1", "2", ""),
                ],
                vec![0.5, 0.25],
            )],
        };
        let text = render_dump(&graph, "GND").unwrap();
        assert!(text.starts_with("GND  tracks: 0.25 mm, 0.5 mm"));
        assert!(text.contains("C1 ----"));
        assert!(text.contains("C1.2 ->"));
    }

    #[test]
    fn a_missing_name_is_an_error() {
        let err = render_dump(&NetGraph::default(), "GND").unwrap_err();
        assert!(err.to_string().contains("GND"));
    }
}
