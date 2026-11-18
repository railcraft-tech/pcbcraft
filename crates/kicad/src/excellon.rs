use anyhow::{bail, Result};
use pcbcraft_domain::drawing::{Hole, Slot};

#[derive(Debug, Clone, PartialEq)]
pub struct Drills {
    pub holes: Vec<Hole>,
    pub slots: Vec<Slot>,
}

pub fn parse(text: &str) -> Result<Drills> {
    let mut tools: Vec<(String, f64)> = Vec::new();
    let mut current: Option<f64> = None;
    let mut holes = Vec::new();
    let mut slots = Vec::new();
    let mut pending: Option<(f64, f64)> = None;
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with(';') || line.starts_with('M') || line == "%" {
            continue;
        }
        if let Some(rest) = line.strip_prefix('T') {
            if let Some((name, diameter)) = rest.split_once('C') {
                let diameter = diameter
                    .trim()
                    .parse::<f64>()
                    .map_err(|_| anyhow::anyhow!("tool diameter {line}"))?;
                tools.push((name.to_string(), diameter));
                continue;
            }
            let id = rest.trim();
            current = tools
                .iter()
                .find(|(name, _)| name == id)
                .map(|(_, diameter)| *diameter);
            if current.is_none() && !id.is_empty() {
                bail!("unknown tool T{id}");
            }
            continue;
        }
        if line.starts_with("G85") {
            let Some((x1, y1)) = pending.take() else {
                bail!("G85 bez poprzedniego otworu");
            };
            let (x2, y2) = xy(line.trim_start_matches("G85"))?;
            let diameter = current.unwrap_or(0.0);
            slots.push(Slot {
                x1,
                y1,
                x2,
                y2,
                diameter,
            });
            continue;
        }
        if line.starts_with('X') || line.starts_with('Y') {
            let (x, y) = xy(line)?;
            let diameter = current.context_diameter()?;
            holes.push(Hole { x, y, diameter });
            pending = Some((x, y));
            continue;
        }
        if line.starts_with('G')
            || line.starts_with("INCH")
            || line.starts_with("METRIC")
            || line.starts_with("FMAT")
        {
            continue;
        }
        bail!("unknown drill record: {line}");
    }
    Ok(Drills { holes, slots })
}

trait Diameter {
    fn context_diameter(self) -> Result<f64>;
}

impl Diameter for Option<f64> {
    fn context_diameter(self) -> Result<f64> {
        self.ok_or_else(|| anyhow::anyhow!("hit without a tool"))
    }
}

fn xy(text: &str) -> Result<(f64, f64)> {
    let mut x = None;
    let mut y = None;
    let bytes = text.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        let mark = bytes[index] as char;
        if mark != 'X' && mark != 'Y' {
            index += 1;
            continue;
        }
        index += 1;
        let start = index;
        while index < bytes.len() && bytes[index] != b'X' && bytes[index] != b'Y' {
            index += 1;
        }
        let number: f64 = std::str::from_utf8(&bytes[start..index])
            .unwrap_or("")
            .trim()
            .parse()
            .map_err(|_| anyhow::anyhow!("coordinate {text}"))?;
        if mark == 'X' {
            x = Some(number);
        } else {
            y = Some(number);
        }
    }
    Ok((
        x.ok_or_else(|| anyhow::anyhow!("missing X in {text}"))?,
        y.ok_or_else(|| anyhow::anyhow!("missing Y in {text}"))?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_decimal_hit() {
        let text = "M48\nMETRIC,TZ\nT01C0.8\n%\nT01\nX1.5Y-2\nM30\n";
        let drills = parse(text).unwrap();
        assert_eq!(drills.holes.len(), 1);
        assert!((drills.holes[0].diameter - 0.8).abs() < 1e-9);
        assert!((drills.holes[0].x - 1.5).abs() < 1e-9);
    }

    #[test]
    fn a_slot_is_kept_and_an_unknown_record_fails() {
        let text = "T1C1.0\n%\nT1\nX0Y0\nG85X4Y0\n";
        let drills = parse(text).unwrap();
        assert_eq!(drills.holes.len(), 1);
        assert_eq!(drills.slots.len(), 1);
        assert!((drills.slots[0].x2 - 4.0).abs() < 1e-9);
        assert!(parse("T1C1.0\n%\nT1\nBOGUS\n").is_err());
    }
}
