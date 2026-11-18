use super::component::Component;
use super::connection::{star, Connection, Pin};

#[derive(Debug, Clone, PartialEq)]
pub struct CopperNet {
    pub name: String,
    pub track_widths_mm: Vec<f64>,
    pub connections: Vec<Connection>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct NetGraph {
    pub components: Vec<Component>,
    pub nets: Vec<CopperNet>,
}

impl NetGraph {
    pub fn named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a CopperNet> + 'a {
        self.nets.iter().filter(move |net| net.name == name)
    }
}

/// One electrical KiCad net: a shared name, its pins, and the track widths on that copper.
pub fn copper_net(name: &str, pins: Vec<Pin>, mut widths: Vec<f64>) -> CopperNet {
    widths.sort_by(f64::total_cmp);
    widths.dedup_by(|left, right| (*left - *right).abs() < 1e-9);
    CopperNet {
        name: name.to_string(),
        track_widths_mm: widths,
        connections: star(name, pins),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::Pin;

    #[test]
    fn duplicate_widths_collapse_and_sort() {
        let net = copper_net(
            "VCC",
            vec![
                Pin {
                    reference: "U1".into(),
                    number: "1".into(),
                    name: "VCC".into(),
                },
                Pin {
                    reference: "C1".into(),
                    number: "1".into(),
                    name: String::new(),
                },
            ],
            vec![0.5, 0.2, 0.5],
        );
        assert_eq!(net.track_widths_mm, vec![0.2, 0.5]);
        assert_eq!(net.connections.len(), 1);
    }
}
