//! Baza korekt wtyczki kicad-jlcpcb-tools.
//!
//! Wtyczka pobiera ten sam CSV (`fetch_remote_corrections` w `library.py`)
//! i trzyma go w SQLite. Tutaj zostaje CSV. Dopasowanie jest takie jak
//! `match_correction`: najpierw dokładny numer LCSC, potem oznaczenie,
//! wartość i footprint. W obrębie jednego pola wygrywa najdłuższe trafienie.

use anyhow::{anyhow, bail, Context, Result};
use fancy_regex::Regex;

use super::{delimiter, header_key, strip_bom};

/// Ten sam adres, którego używa kicad-jlcpcb-tools.
pub const CORRECTIONS_URL: &str =
    "https://raw.githubusercontent.com/matthewlai/JLCKicadTools/master/jlc_kicad_tools/cpl_rotations_db.csv";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchField {
    Lcsc,
    Reference,
    Value,
    Footprint,
    LocalRef,
    LocalFootprint,
}

impl std::fmt::Display for MatchField {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Lcsc => "LCSC",
            Self::Reference => "reference",
            Self::Value => "value",
            Self::Footprint => "footprint",
            Self::LocalRef => "reference, local file",
            Self::LocalFootprint => "footprint, local file",
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Hit {
    pub key: String,
    pub field: MatchField,
    pub rotation: i64,
    pub offset_x: f64,
    pub offset_y: f64,
    /// Przesunięcie w osiach pliku pozycji. Baza sieciowa obraca je z footprintem.
    pub board_offset: bool,
}

struct PatternRule {
    key: String,
    rotation: i64,
    offset_x: f64,
    offset_y: f64,
    regex: Regex,
}

struct LcscRule {
    lcsc: String,
    rotation: i64,
    offset_x: f64,
    offset_y: f64,
}

pub struct Database {
    lcsc: Vec<LcscRule>,
    patterns: Vec<PatternRule>,
}

impl Database {
    pub fn parse(text: &str) -> Result<Self> {
        let text = strip_bom(text);
        if text.trim().is_empty() {
            bail!("correction database is empty");
        }
        let mut reader = csv::ReaderBuilder::new()
            .delimiter(delimiter(text))
            .flexible(true)
            .from_reader(text.as_bytes());
        let headers = reader
            .headers()
            .context("correction database header")?
            .iter()
            .map(clean_header)
            .collect::<Vec<_>>();
        let cols = db_columns(&headers)?;

        let mut db = Database {
            lcsc: Vec::new(),
            patterns: Vec::new(),
        };
        for record in reader.records() {
            let record = record.context("correction database")?;
            let line = record.position().map(|pos| pos.line()).unwrap_or(0);
            if record.iter().all(|cell| cell.trim().is_empty()) {
                continue;
            }
            db.push_row(&cols, &record)
                .with_context(|| format!("correction database, line {line}"))?;
        }
        if db.is_empty() {
            bail!("correction database has no rules");
        }
        Ok(db)
    }

    pub fn len(&self) -> usize {
        self.lcsc.len() + self.patterns.len()
    }

    pub fn is_empty(&self) -> bool {
        self.lcsc.is_empty() && self.patterns.is_empty()
    }

    pub fn overlay(&mut self, extra: Database) {
        for rule in extra.lcsc {
            upsert_lcsc(&mut self.lcsc, rule);
        }
        for rule in extra.patterns {
            upsert_pattern(&mut self.patterns, rule);
        }
    }

    pub fn lookup(
        &self,
        reference: &str,
        value: &str,
        package: &str,
        lcsc: &str,
    ) -> Result<Option<Hit>> {
        if let Some(key) = exact_lcsc(lcsc) {
            if let Some(rule) = self.lcsc.iter().find(|rule| rule.lcsc == key) {
                return Ok(Some(Hit {
                    key: rule.lcsc.clone(),
                    field: MatchField::Lcsc,
                    rotation: rule.rotation,
                    offset_x: rule.offset_x,
                    offset_y: rule.offset_y,
                    board_offset: false,
                }));
            }
        }

        let package = footprint_item(package);
        for (field, text) in [
            (MatchField::Reference, reference.trim()),
            (MatchField::Value, value.trim()),
            (MatchField::Footprint, package),
        ] {
            if let Some(rule) = longest(&self.patterns, text)? {
                return Ok(Some(Hit {
                    key: rule.key.clone(),
                    field,
                    rotation: rule.rotation,
                    offset_x: rule.offset_x,
                    offset_y: rule.offset_y,
                    board_offset: false,
                }));
            }
        }
        Ok(None)
    }

    fn push_row(&mut self, cols: &DbCols, record: &csv::StringRecord) -> Result<()> {
        let pattern = field(record, Some(cols.pattern)).trim().to_string();
        if pattern.is_empty() {
            bail!("pusty wzorzec");
        }
        let rotation = parse_whole_degrees(field(record, Some(cols.rotation)))?;
        let offset_x = parse_offset(field(record, cols.offset_x))?;
        let offset_y = parse_offset(field(record, cols.offset_y))?;

        if let Some(lcsc) = lcsc_key(&pattern) {
            upsert_lcsc(
                &mut self.lcsc,
                LcscRule {
                    lcsc,
                    rotation,
                    offset_x,
                    offset_y,
                },
            );
            return Ok(());
        }

        let source = python_carets(&pattern);
        let regex = Regex::new(&source).with_context(|| format!("wzorzec „{pattern}”"))?;
        upsert_pattern(
            &mut self.patterns,
            PatternRule {
                key: pattern,
                rotation,
                offset_x,
                offset_y,
                regex,
            },
        );
        Ok(())
    }
}

/// Oficjalny plik ma spację przed cudzysłowem ostatniej kolumny
/// (`"Offset X", "Offset Y"`). Parser traktuje wtedy cudzysłów jako treść.
fn clean_header(name: &str) -> String {
    name.trim().trim_matches('"').trim().to_string()
}

struct DbCols {
    pattern: usize,
    rotation: usize,
    offset_x: Option<usize>,
    offset_y: Option<usize>,
}

fn db_columns(headers: &[String]) -> Result<DbCols> {
    let mut pattern = None;
    let mut rotation = None;
    let mut offset_x = None;
    let mut offset_y = None;
    for (idx, header) in headers.iter().enumerate() {
        if header.trim().is_empty() {
            bail!("empty column name in the database header");
        }
        match header_key(header).as_str() {
            "pattern" | "footprint pattern" | "regex" => {
                assign(&mut pattern, idx, header)?;
            }
            "rotation" | "correction" => assign(&mut rotation, idx, header)?,
            "offset x" | "offsetx" => assign(&mut offset_x, idx, header)?,
            "offset y" | "offsety" => assign(&mut offset_y, idx, header)?,
            other => bail!("unknown database column '{other}'"),
        }
    }
    let pattern = pattern.context("database header is missing a pattern column")?;
    let rotation = rotation.context("database header is missing a rotation column")?;
    Ok(DbCols {
        pattern,
        rotation,
        offset_x,
        offset_y,
    })
}

fn assign(slot: &mut Option<usize>, idx: usize, name: &str) -> Result<()> {
    if slot.replace(idx).is_some() {
        bail!("repeated column \"{name}\"");
    }
    Ok(())
}

fn field(record: &csv::StringRecord, idx: Option<usize>) -> &str {
    idx.and_then(|idx| record.get(idx)).unwrap_or("")
}

fn longest<'a>(rules: &'a [PatternRule], text: &str) -> Result<Option<&'a PatternRule>> {
    let mut best: Option<&PatternRule> = None;
    let mut best_len = 0usize;
    for rule in rules {
        let matched = rule
            .regex
            .find(text)
            .with_context(|| format!("wzorzec „{}”", rule.key))?;
        if let Some(found) = matched {
            let len = found.as_str().chars().count();
            if best.is_none() || len > best_len {
                best = Some(rule);
                best_len = len;
            }
        }
    }
    Ok(best)
}

fn upsert_pattern(list: &mut Vec<PatternRule>, rule: PatternRule) {
    if let Some(slot) = list.iter_mut().find(|existing| existing.key == rule.key) {
        *slot = rule;
    } else {
        list.push(rule);
    }
}

fn upsert_lcsc(list: &mut Vec<LcscRule>, rule: LcscRule) {
    if let Some(slot) = list.iter_mut().find(|existing| existing.lcsc == rule.lcsc) {
        *slot = rule;
    } else {
        list.push(rule);
    }
}

/// Nazwa footprintu bez prefiksu biblioteki (`Lib:Nazwa` → `Nazwa`).
pub(crate) fn footprint_item(package: &str) -> &str {
    let package = package.trim();
    match package.rsplit_once(':') {
        Some((_, name)) if !name.trim().is_empty() => name.trim(),
        _ => package,
    }
}

pub(crate) fn is_lcsc_part(value: &str) -> bool {
    let value = normalize_lcsc(value);
    let mut chars = value.chars();
    chars.next() == Some('C')
        && chars.clone().next().is_some()
        && chars.all(|c| c.is_ascii_digit())
        && value.is_ascii()
}

fn normalize_lcsc(value: &str) -> String {
    value.trim().to_ascii_uppercase()
}

fn exact_lcsc(value: &str) -> Option<String> {
    if is_lcsc_part(value) {
        Some(normalize_lcsc(value))
    } else {
        None
    }
}

/// Sam numer LCSC, ewentualnie zakotwiczony `^C12345$`, jest regułą części,
/// a nie wzorcem footprintu. Takie reguły wtyczka trzyma osobno i sprawdza pierwsze.
fn lcsc_key(pattern: &str) -> Option<String> {
    let trimmed = pattern.trim();
    let inner = trimmed.strip_prefix('^').unwrap_or(trimmed);
    let inner = inner.strip_suffix('$').unwrap_or(inner);
    if inner.contains(['\\', '.', '*', '+', '?', '[', '(', '|', '^', '$']) {
        return None;
    }
    exact_lcsc(inner)
}

/// W Pythonowym `re` drugi `^` na początku wzorca jest znakiem, nie kotwicą.
/// W bazie jest literówka `^^WSON-…`; bez tej zamiany Rust dopasowałby WSON.
fn python_carets(pattern: &str) -> String {
    match pattern.strip_prefix("^^") {
        Some(rest) => format!(r"^\^{rest}"),
        None => pattern.to_string(),
    }
}

fn parse_whole_degrees(text: &str) -> Result<i64> {
    let text = text.trim();
    if text.is_empty() {
        bail!("missing angle");
    }
    if let Ok(value) = text.parse::<i64>() {
        return Ok(value);
    }
    let float: f64 = text
        .parse()
        .map_err(|_| anyhow!("angle \"{text}\" is not a number"))?;
    if !float.is_finite() {
        bail!("angle \"{text}\" is not a number");
    }
    let rounded = float.round();
    if (float - rounded).abs() > 1e-6 || !(i64::MIN as f64..=i64::MAX as f64).contains(&rounded) {
        bail!("angle \"{text}\" must be a whole number of degrees");
    }
    Ok(rounded as i64)
}

fn parse_offset(text: &str) -> Result<f64> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(0.0);
    }
    let value: f64 = text
        .parse()
        .map_err(|_| anyhow!("offset \"{text}\" is not a number"))?;
    if !value.is_finite() {
        bail!("offset \"{text}\" is not a number");
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn load(csv: &str) -> Database {
        Database::parse(csv).unwrap()
    }

    fn rotation(db: &Database, package: &str) -> i64 {
        db.lookup("U1", "X", package, "").unwrap().unwrap().rotation
    }

    #[test]
    fn official_header_and_short_rows() {
        let csv = "\
\"Footprint pattern\",\"Rotation\",\"Offset X\", \"Offset Y\"
\"^SOT-23\",-90
\"^USB_C_Receptacle_XKB_U262-16XN-4BVC11\",0,1.44,0
";
        let db = load(csv);
        assert_eq!(db.len(), 2);
        let sot = db.lookup("Q1", "AO3401A", "SOT-23", "").unwrap().unwrap();
        assert_eq!(sot.rotation, -90);
        assert_eq!(sot.offset_x, 0.0);
        assert_eq!(sot.offset_y, 0.0);
        let usb = db
            .lookup("J1", "USB", "USB_C_Receptacle_XKB_U262-16XN-4BVC11", "")
            .unwrap()
            .unwrap();
        assert_eq!(usb.rotation, 0);
        assert_eq!(usb.offset_x, 1.44);
        assert_eq!(usb.offset_y, 0.0);
    }

    #[test]
    fn longest_match_and_field_order() {
        let mut db = load("pattern,rotation\n^SOT-23,-90\n");
        db.overlay(load("pattern,rotation\n^SOT-23-5,180\n"));
        assert_eq!(rotation(&db, "SOT-23-5"), 180);
        assert_eq!(rotation(&db, "SOT-23"), -90);
        assert_eq!(rotation(&db, "Package_TO_SOT_SMD:SOT-23-3"), -90);

        let db = load("pattern,rotation\n^SOT-23,-90\n^Q1$,10\n");
        let hit = db.lookup("Q1", "X", "SOT-23", "").unwrap().unwrap();
        assert_eq!(hit.field, MatchField::Reference);
        assert_eq!(hit.rotation, 10);

        let db = load("pattern,rotation\n^SPECIAL,15\n^SOT-23,-90\n");
        let hit = db
            .lookup("Q1", "SPECIAL-123", "SOT-23", "")
            .unwrap()
            .unwrap();
        assert_eq!(hit.field, MatchField::Value);
        assert_eq!(hit.rotation, 15);
    }

    #[test]
    fn lcsc_rule_outranks_a_footprint_pattern() {
        let db = load("pattern,rotation\nC99999,45\n^SOT-23,-90\n");
        let hit = db.lookup("Q1", "X", "SOT-23", "c99999").unwrap().unwrap();
        assert_eq!(hit.field, MatchField::Lcsc);
        assert_eq!(hit.key, "C99999");
        assert_eq!(hit.rotation, 45);
        assert!(db.lookup("Q1", "X", "C99999", "").unwrap().is_none());
    }

    #[test]
    fn upstream_patterns_that_need_fancy_regex() {
        let db = load("pattern,rotation\n^SOP-4_,0\n^SOP-(?!18_),270\n^SOP-18_,0\n");
        assert_eq!(rotation(&db, "SOP-4_3.9x4.9mm"), 0);
        assert_eq!(rotation(&db, "SOP-8_3.9x4.9mm"), 270);
        assert_eq!(rotation(&db, "SOP-18_5.3x11.6mm"), 0);

        let db = load("pattern,rotation\n^(.*?_|V)?QFN-(16|20|24|28|40)(-|_|$),270\n");
        assert_eq!(rotation(&db, "QFN-40-1EP_5x5mm_P0.4mm"), 270);
        assert!(db.lookup("U1", "X", "QFN-48-1EP", "").unwrap().is_none());
    }

    #[test]
    fn doubled_caret_stays_a_literal_like_python() {
        let db = load("pattern,rotation\n^^WSON-8-1EP_6x5mm_P1.27mm,-90\n");
        assert!(db
            .lookup("U1", "X", "WSON-8-1EP_6x5mm_P1.27mm", "")
            .unwrap()
            .is_none());
        assert!(db
            .lookup("U1", "X", "^WSON-8-1EP_6x5mm_P1.27mm", "")
            .unwrap()
            .is_some());
    }

    #[test]
    fn rejects_a_fractional_angle_and_a_broken_regex() {
        assert!(Database::parse("pattern,rotation\n^OK,90.5\n").is_err());
        assert!(Database::parse("pattern,rotation\n[,90\n").is_err());
        assert!(Database::parse("rotation\n90\n").is_err());
        assert!(Database::parse("").is_err());
    }

    #[test]
    fn equal_length_keeps_the_earlier_rule() {
        let db = load("pattern,rotation\n^AB,1\n^A.,2\n");
        let hit = db.lookup("R1", "AB", "ZZZ", "").unwrap().unwrap();
        assert_eq!(hit.rotation, 1);
    }
}
