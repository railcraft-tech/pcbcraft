use std::collections::HashMap;

use pcbcraft_domain::net::{copper_net, Component, NetGraph, Pin};

#[derive(Debug, Clone)]
pub struct PinGroup {
    pub name: String,
    pub pins: Vec<Pin>,
}

pub fn assemble(
    components: Vec<Component>,
    groups: Vec<PinGroup>,
    widths: &HashMap<String, Vec<f64>>,
) -> NetGraph {
    let nets = groups
        .into_iter()
        .map(|group| {
            let track_widths = widths.get(&group.name).cloned().unwrap_or_default();
            copper_net(&group.name, group.pins, track_widths)
        })
        .collect();
    NetGraph { components, nets }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pin(reference: &str, number: &str) -> Pin {
        Pin {
            reference: reference.into(),
            number: number.into(),
            name: String::new(),
        }
    }

    #[test]
    fn the_same_name_stays_two_stars() {
        let graph = assemble(
            Vec::new(),
            vec![
                PinGroup {
                    name: "GND".into(),
                    pins: vec![pin("R1", "2"), pin("C1", "2")],
                },
                PinGroup {
                    name: "GND".into(),
                    pins: vec![pin("R2", "2"), pin("C2", "2")],
                },
            ],
            &HashMap::new(),
        );
        assert_eq!(graph.nets.len(), 2);
        assert_eq!(graph.named("GND").count(), 2);
        assert!(graph.nets.iter().all(|net| net.connections.len() == 1));
    }
}
