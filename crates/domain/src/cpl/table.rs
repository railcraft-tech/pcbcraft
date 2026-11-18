//! Odczyt pliku pozycji KiCada i BOM-u z numerami LCSC.

use std::collections::HashMap;

use anyhow::{anyhow, bail, Context, Result};

use super::{delimiter, header_key, strip_bom};

#[derive(Debug)]
pub(crate) struct Columns {
    pub designator: usize,
    pub value: Option<usize>,
    pub package: usize,
    pub x: usize,
    pub y: usize,
    pub rotation: usize,
    pub layer: usize,
    pub lcsc: Option<usize>,
    pub extras: Vec<usize>,
}

#[derive(Debug)]
pub(crate) struct InputRow {
    pub line: u64,
    pub cells: Vec<String>,
}

#[derive(Debug)]
pub struct PosFile {
    pub(crate) header: Vec<String>,
    pub(crate) cols: Columns,
    pub(crate) rows: Vec<InputRow>,
}

impl PosFile {
    pub fn parse(text: &str) -> Result<Self> {
        let text = strip_bom(text);
        if text.trim().is_empty() {
            bail!("position file jest pusty");
        }
        if text.trim_start().starts_with('#') {
            bail!(
                "this is a text .pos file, not CSV. Choose CSV when exporting positions from KiCad."
            );
        }
        let mut reader = csv::ReaderBuilder::new()
            .delimiter(delimiter(text))
            .flexible(true)
            .from_reader(text.as_bytes());
        let header = reader
            .headers()
            .context("position file header")?
            .iter()
            .map(str::to_string)
            .collect::<Vec<_>>();
        let cols = pos_columns(&header)?;
        let width = header.len();
        let mut rows = Vec::new();
        for record in reader.records() {
            let record = record.context("position file")?;
            let line = record.position().map(|pos| pos.line()).unwrap_or(0);
            if record.iter().all(|cell| cell.trim().is_empty()) {
                continue;
            }
            let mut cells = record.iter().map(str::to_string).collect::<Vec<_>>();
            cells.resize(width, String::new());
            rows.push(InputRow { line, cells });
        }
        if rows.is_empty() {
            bail!("position file has no parts");
        }
        Ok(PosFile { header, cols, rows })
    }
}

pub struct Cpl {
    header: Vec<String>,
    rows: Vec<Vec<String>>,
}

impl Cpl {
    pub(crate) fn new(header: Vec<String>, rows: Vec<Vec<String>>) -> Self {
        Self { header, rows }
    }

    pub fn to_csv(&self) -> Result<String> {
        let mut buf = Vec::new();
        {
            let mut writer = csv::WriterBuilder::new()
                .terminator(csv::Terminator::Any(b'\n'))
                .from_writer(&mut buf);
            writer.write_record(&self.header)?;
            for row in &self.rows {
                writer.write_record(row)?;
            }
            writer.flush()?;
        }
        String::from_utf8(buf).context("zapis CSV")
    }
}

/// Mapa oznaczenie → numer LCSC. Klucze są wielkimi literami.
/// Drugi element to ostrzeżenia, które nie przerywają korekty.
pub fn parse_bom(text: &str) -> Result<(HashMap<String, String>, Vec<String>)> {
    let text = strip_bom(text);
    if text.trim().is_empty() {
        bail!("BOM jest pusty");
    }
    let mut reader = csv::ReaderBuilder::new()
        .delimiter(delimiter(text))
        .flexible(true)
        .from_reader(text.as_bytes());
    let headers = reader
        .headers()
        .context("BOM header")?
        .iter()
        .map(str::to_string)
        .collect::<Vec<_>>();
    let designator = find_column(&headers, &["ref", "designator", "reference"])
        .context("BOM is missing a Designator column")?;
    let lcsc = find_column(&headers, &["lcsc", "lcsc part", "lcsc part number"])
        .context("BOM is missing an LCSC column")?;

    let mut map = HashMap::new();
    let mut warnings = Vec::new();
    for record in reader.records() {
        let record = record.context("BOM")?;
        let line = record.position().map(|pos| pos.line()).unwrap_or(0);
        let parts = record.get(lcsc).unwrap_or("").trim();
        if parts.is_empty() {
            continue;
        }
        if !super::db::is_lcsc_part(parts) {
            warnings.push(format!("BOM, line {line}: '{parts}' is not an LCSC number"));
            continue;
        }
        let lcsc_id = parts.trim().to_ascii_uppercase();
        let designators = record.get(designator).unwrap_or("");
        for designator in designators.split(',') {
            let designator = designator.trim();
            if designator.is_empty() {
                continue;
            }
            let key = designator.to_ascii_uppercase();
            if let Some(previous) = map.insert(key, lcsc_id.clone()) {
                if previous != lcsc_id {
                    warnings.push(format!(
                        "BOM, line {line}: {designator} has LCSC {previous} and {lcsc_id}; keeping {lcsc_id}"
                    ));
                }
            }
        }
    }
    Ok((map, warnings))
}

fn find_column(headers: &[String], names: &[&str]) -> Option<usize> {
    headers.iter().position(|header| {
        let key = header_key(header);
        names.contains(&key.as_str())
    })
}

fn pos_columns(headers: &[String]) -> Result<Columns> {
    let mut designator = None;
    let mut value = None;
    let mut comment = None;
    let mut package = None;
    let mut x = None;
    let mut y = None;
    let mut rotation = None;
    let mut layer = None;
    let mut lcsc = None;

    for (idx, header) in headers.iter().enumerate() {
        if header.trim().is_empty() {
            bail!("empty column name in the position file");
        }
        match header_key(header).as_str() {
            "ref" | "designator" | "reference" => assign(&mut designator, idx, header)?,
            "val" | "value" => assign(&mut value, idx, header)?,
            "comment" => assign(&mut comment, idx, header)?,
            "package" | "footprint" | "footprint name" | "pkg" => {
                assign(&mut package, idx, header)?;
            }
            "posx" | "pos x" | "mid x" => assign(&mut x, idx, header)?,
            "posy" | "pos y" | "mid y" => assign(&mut y, idx, header)?,
            "rot" | "rotation" | "orientation" | "orient" => assign(&mut rotation, idx, header)?,
            "side" | "layer" => assign(&mut layer, idx, header)?,
            "lcsc" | "lcsc part" | "lcsc part number" => assign(&mut lcsc, idx, header)?,
            _ => {}
        }
    }

    let mut missing = Vec::new();
    if designator.is_none() {
        missing.push("Designator");
    }
    if package.is_none() {
        missing.push("Package");
    }
    if x.is_none() {
        missing.push("PosX / Mid X");
    }
    if y.is_none() {
        missing.push("PosY / Mid Y");
    }
    if rotation.is_none() {
        missing.push("Rot / Rotation");
    }
    if layer.is_none() {
        missing.push("Side / Layer");
    }
    if !missing.is_empty() {
        bail!(
            "header is missing: {}. Export a KiCad position CSV (Ref, Val, Package, PosX, PosY, Rot, Side) or a CPL with a Package column.",
            missing.join(", ")
        );
    }

    let value = value.or(comment);
    let used = [designator, value, package, x, y, rotation, layer]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    // Komentarz, którego nie wzięliśmy jako wartości, zostaje kolumną dodatkową.
    let extras = (0..headers.len())
        .filter(|idx| !used.contains(idx))
        .collect();

    Ok(Columns {
        designator: designator.expect("sprawdzone"),
        value,
        package: package.expect("sprawdzone"),
        x: x.expect("sprawdzone"),
        y: y.expect("sprawdzone"),
        rotation: rotation.expect("sprawdzone"),
        layer: layer.expect("sprawdzone"),
        lcsc,
        extras,
    })
}

fn assign(slot: &mut Option<usize>, idx: usize, name: &str) -> Result<()> {
    if slot.replace(idx).is_some() {
        bail!("repeated column '{name}'");
    }
    Ok(())
}

pub(crate) fn cell(row: &InputRow, idx: usize) -> &str {
    row.cells.get(idx).map(String::as_str).unwrap_or("").trim()
}

pub(crate) fn parse_mm(text: &str, what: &str) -> Result<f64> {
    let text = text.trim();
    let value: f64 = text
        .parse()
        .map_err(|_| anyhow!("{what} '{text}' is not a number"))?;
    if !value.is_finite() {
        bail!("{what} '{text}' is not a number");
    }
    Ok(value)
}

pub(crate) fn parse_side(text: &str) -> Option<bool> {
    match text.trim().to_ascii_lowercase().as_str() {
        "bottom" | "back" | "bot" | "b.cu" => Some(true),
        "top" | "front" | "t" | "f.cu" => Some(false),
        _ => None,
    }
}

pub(crate) fn layer_text(original: &str, bottom: bool) -> String {
    let trimmed = original.trim();
    if trimmed.eq_ignore_ascii_case("top") || trimmed.eq_ignore_ascii_case("bottom") {
        trimmed.to_string()
    } else if bottom {
        "bottom".to_string()
    } else {
        "top".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_a_position_file_without_a_package_column() {
        let err =
            PosFile::parse("Designator,Mid X,Mid Y,Layer,Rotation\nA,1,2,top,0\n").unwrap_err();
        assert!(err.to_string().contains("Package"), "{err}");
    }

    #[test]
    fn rejects_the_ascii_pos_preamble() {
        let err = PosFile::parse("### Module positions\n## Unit = mm\n").unwrap_err();
        assert!(err.to_string().contains("CSV"), "{err}");
    }

    #[test]
    fn bom_splits_grouped_designators() {
        let csv = "\
Designator,Comment,Footprint,LCSC Part #
\"Q1,Q2\",BC857,Package_TO_SOT_SMD:SOT-23,C12345
R1,10k,R_0402,nie-lcsc
C1,100n,C_0402,
";
        let (map, warnings) = parse_bom(csv).unwrap();
        assert_eq!(map.get("Q1").map(String::as_str), Some("C12345"));
        assert_eq!(map.get("Q2").map(String::as_str), Some("C12345"));
        assert!(!map.contains_key("R1"));
        assert!(!map.contains_key("C1"));
        assert_eq!(warnings.len(), 1);
    }
}
