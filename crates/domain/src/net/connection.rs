#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pin {
    pub reference: String,
    pub number: String,
    pub name: String,
}

impl Pin {
    /// Name when KiCad stored one, otherwise the pad number.
    pub fn label(&self) -> &str {
        let name = self.name.trim();
        if name.is_empty() || name == "~" {
            self.number.as_str()
        } else {
            name
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Connection {
    pub name: String,
    pub from: Pin,
    pub to: Pin,
}

fn pin_ord(pin: &Pin) -> (String, u8, u64, String) {
    let number = pin.number.trim();
    if let Ok(value) = number.parse::<u64>() {
        (pin.reference.to_ascii_uppercase(), 0, value, String::new())
    } else {
        (
            pin.reference.to_ascii_uppercase(),
            1,
            0,
            number.to_ascii_uppercase(),
        )
    }
}

/// Pins of one electrical net become a star. The hub is the smallest reference and pin number.
pub fn star(name: &str, mut pins: Vec<Pin>) -> Vec<Connection> {
    pins.sort_by(|left, right| pin_ord(left).cmp(&pin_ord(right)));
    pins.dedup_by(|left, right| left.reference == right.reference && left.number == right.number);
    let Some(hub) = pins.first().cloned() else {
        return Vec::new();
    };
    pins.into_iter()
        .skip(1)
        .map(|pin| Connection {
            name: name.to_string(),
            from: hub.clone(),
            to: pin,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pin(reference: &str, number: &str, name: &str) -> Pin {
        Pin {
            reference: reference.into(),
            number: number.into(),
            name: name.into(),
        }
    }

    #[test]
    fn star_hub_is_the_smallest_pin() {
        let arms = star(
            "GND",
            vec![
                pin("U1", "3", "GND"),
                pin("R1", "2", ""),
                pin("C1", "2", ""),
            ],
        );
        assert_eq!(arms.len(), 2);
        assert!(arms.iter().all(|arm| arm.name == "GND"));
        assert!(arms.iter().all(|arm| arm.from.reference == "C1"));
    }
}
