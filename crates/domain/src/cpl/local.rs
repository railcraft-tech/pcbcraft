//! Lokalna correction database w YAML.
//!
//! Reguła stąd zastępuje trafienie z bazy kicad-jlcpcb-tools.
//! Kąt dodaje się tak samo: po odbiciu spodu. Przesunięcie jest różnicą
//! w osiach pliku pozycji i nie obraca się razem z częścią.

use anyhow::{anyhow, bail, Context, Result};
use serde::Deserialize;

use super::db::{footprint_item, Hit, MatchField};
use super::strip_bom;

#[derive(Debug, Clone, PartialEq, Default)]
pub struct LocalDb {
    rules: Vec<LocalRule>,
}

#[derive(Debug, Clone, PartialEq)]
struct LocalRule {
    kind: LocalKind,
    key: String,
    rotation: i64,
    x: f64,
    y: f64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum LocalKind {
    Ref,
    Footprint,
}

impl LocalDb {
    pub fn parse(text: &str) -> Result<Self> {
        let text = strip_bom(text);
        if text.trim().is_empty() {
            bail!("lokalna baza jest pusta");
        }
        let file: LocalFile = serde_yaml::from_str(text).context("lokalna baza YAML")?;
        let raw = file.transformations;
        if raw.is_empty() {
            bail!("local database has no rules");
        }
        let mut rules = Vec::with_capacity(raw.len());
        for (index, rule) in raw.into_iter().enumerate() {
            rules.extend(
                rule.into_rules()
                    .with_context(|| format!("rule {}", index + 1))?,
            );
        }
        let rules = dedup(rules)?;
        Ok(Self { rules })
    }

    pub fn len(&self) -> usize {
        self.rules.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    /// Oznaczenie wygrywa z footprintem. Przy footprintach wygrywa dłuższy prefiks.
    pub fn lookup(&self, designator: &str, package: &str) -> Option<usize> {
        let designator = designator.trim();
        if let Some(index) = self.rules.iter().position(|rule| {
            rule.kind == LocalKind::Ref && rule.key.eq_ignore_ascii_case(designator)
        }) {
            return Some(index);
        }
        let name = footprint_item(package);
        let mut best: Option<(usize, usize)> = None;
        for (index, rule) in self.rules.iter().enumerate() {
            if rule.kind != LocalKind::Footprint || !footprint_prefix(&rule.key, name) {
                continue;
            }
            let len = rule.key.chars().count();
            if best.is_none_or(|(_, best_len)| len > best_len) {
                best = Some((index, len));
            }
        }
        best.map(|(index, _)| index)
    }

    pub fn hit(&self, index: usize) -> Hit {
        let rule = &self.rules[index];
        Hit {
            key: rule.key.clone(),
            field: match rule.kind {
                LocalKind::Ref => MatchField::LocalRef,
                LocalKind::Footprint => MatchField::LocalFootprint,
            },
            rotation: rule.rotation,
            offset_x: rule.x,
            offset_y: rule.y,
            board_offset: true,
        }
    }

    pub fn label(&self, index: usize) -> String {
        let rule = &self.rules[index];
        match rule.kind {
            LocalKind::Ref => format!("ref {}", rule.key),
            LocalKind::Footprint => format!("footprint {}", rule.key),
        }
    }
}

/// `SOT-23` pasuje do `SOT-23` i `SOT-23-3`, nie do `SOT-223`.
fn footprint_prefix(pattern: &str, name: &str) -> bool {
    if name == pattern {
        return true;
    }
    let Some(rest) = name.strip_prefix(pattern) else {
        return false;
    };
    rest.starts_with('-') || rest.starts_with('_')
}

fn dedup(rules: Vec<LocalRule>) -> Result<Vec<LocalRule>> {
    let mut kept: Vec<LocalRule> = Vec::with_capacity(rules.len());
    for rule in rules {
        if let Some(earlier) = kept
            .iter()
            .find(|other| other.kind == rule.kind && other.key.eq_ignore_ascii_case(&rule.key))
        {
            if earlier.rotation != rule.rotation || earlier.x != rule.x || earlier.y != rule.y {
                bail!(
                    "repeated rule {} {} disagrees with an earlier correction",
                    match earlier.kind {
                        LocalKind::Ref => "ref",
                        LocalKind::Footprint => "footprint",
                    },
                    earlier.key
                );
            }
            continue;
        }
        kept.push(rule);
    }
    Ok(kept)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LocalFile {
    transformations: Vec<RawRule>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRule {
    #[serde(rename = "ref", default)]
    reference: Option<OneOrMany>,
    #[serde(default)]
    footprint: Option<OneOrMany>,
    #[serde(default)]
    rotation: Option<f64>,
    #[serde(default)]
    x: Option<f64>,
    #[serde(default)]
    y: Option<f64>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum OneOrMany {
    One(String),
    Many(Vec<String>),
}

impl RawRule {
    fn into_rules(self) -> Result<Vec<LocalRule>> {
        let references = names(self.reference)?;
        let footprints = names(self.footprint)?;
        let (kind, keys) = match (references.is_empty(), footprints.is_empty()) {
            (false, true) => (LocalKind::Ref, references),
            (true, false) => (LocalKind::Footprint, footprints),
            (false, false) => bail!("give either ref or footprint, not both"),
            (true, true) => bail!("rule has neither ref nor footprint"),
        };
        let specifies_rotation = self.rotation.is_some();
        let rotation = match self.rotation {
            Some(value) => whole_degrees(value)?,
            None => 0,
        };
        let x = finite("x", self.x)?;
        let y = finite("y", self.y)?;
        if !specifies_rotation && x == 0.0 && y == 0.0 {
            bail!("rule changes neither angle nor position");
        }
        Ok(keys
            .into_iter()
            .map(|key| LocalRule {
                kind: kind.clone(),
                key,
                rotation,
                x,
                y,
            })
            .collect())
    }
}

fn names(value: Option<OneOrMany>) -> Result<Vec<String>> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let items = match value {
        OneOrMany::One(text) => vec![text],
        OneOrMany::Many(items) => {
            if items.is_empty() {
                bail!("pusta tablica");
            }
            items
        }
    };
    let mut names = Vec::with_capacity(items.len());
    for item in items {
        let item = item.trim();
        if item.is_empty() {
            bail!("pusta nazwa w tablicy");
        }
        names.push(item.to_string());
    }
    Ok(names)
}

fn whole_degrees(value: f64) -> Result<i64> {
    if !value.is_finite() {
        bail!("angle is not a number");
    }
    let rounded = value.round();
    if (value - rounded).abs() > 1e-6 || !(i64::MIN as f64..=i64::MAX as f64).contains(&rounded) {
        bail!("angle {value} must be a whole number of degrees");
    }
    Ok(rounded as i64)
}

fn finite(name: &str, value: Option<f64>) -> Result<f64> {
    match value {
        None => Ok(0.0),
        Some(value) if value.is_finite() => Ok(value),
        Some(_) => Err(anyhow!("offset {name} is not a number")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ref_beats_a_shorter_footprint_prefix() {
        let db = LocalDb::parse(
            "\
transformations:
  - footprint: SOT-23
    rotation: -90
  - ref: U1
    rotation: 90
  - footprint: SOT-23-5
    rotation: 180
",
        )
        .unwrap();
        assert_eq!(db.hit(db.lookup("U1", "SOT-23").unwrap()).rotation, 90);
        assert_eq!(db.hit(db.lookup("Q1", "SOT-23").unwrap()).rotation, -90);
        assert_eq!(db.hit(db.lookup("Q2", "SOT-23-3").unwrap()).rotation, -90);
        assert_eq!(db.hit(db.lookup("U7", "SOT-23-5").unwrap()).rotation, 180);
        assert!(db.lookup("U9", "SOT-223").is_none());
    }

    #[test]
    fn usb_prefix_stops_at_a_boundary() {
        let db = LocalDb::parse("transformations:\n  - footprint: USB\n    x: 1.26\n").unwrap();
        assert!(db.lookup("J1", "USB_C_Receptacle_GCT").is_some());
        assert!(db.lookup("D1", "SOT-23-6").is_none());
        let hit = db.hit(db.lookup("J1", "Lib:USB_C_Receptacle").unwrap());
        assert_eq!(hit.offset_x, 1.26);
        assert!(hit.board_offset);
    }

    #[test]
    fn rejects_a_duplicate_and_a_fractional_angle() {
        assert!(LocalDb::parse(
            "transformations:\n  - ref: U1\n    rotation: 90\n  - ref: u1\n    x: 1\n"
        )
        .is_err());
        assert!(LocalDb::parse("transformations:\n  - ref: U1\n    rotation: 90.5\n").is_err());
        assert!(LocalDb::parse("transformations:\n  - ref: U1\n").is_err());
        assert!(LocalDb::parse("- ref: U1\n  rotation: 90\n").is_err());
        assert!(LocalDb::parse("transformations:\n  - ref: []\n    rotation: 90\n").is_err());
    }

    #[test]
    fn arrays_apply_one_correction_to_each_name() {
        let db = LocalDb::parse(
            "\
transformations:
  - ref: [Q1, Q2]
    rotation: -90
  - footprint: [SOT-23, USB]
    x: 1.26
  - ref: [U7]
    rotation: -90
  - ref: [Q4, U7]
    rotation: -90
",
        )
        .unwrap();
        assert_eq!(db.hit(db.lookup("Q1", "SOT-23").unwrap()).rotation, -90);
        assert_eq!(db.hit(db.lookup("Q2", "inny").unwrap()).rotation, -90);
        assert_eq!(db.hit(db.lookup("Q4", "inny").unwrap()).rotation, -90);
        assert_eq!(
            db.hit(db.lookup("J1", "USB_C_Receptacle").unwrap())
                .offset_x,
            1.26
        );
        assert_eq!(db.hit(db.lookup("R1", "SOT-23-3").unwrap()).offset_x, 1.26);
        assert_eq!(db.len(), 6);
    }
}
