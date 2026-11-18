//! Five-column JLCPCB CPL on top of the jlc-cpl correction engine.

use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::{Context, Result};
use pcbcraft_domain::cpl::{correct, parse_bom, Database, LocalDb, PosFile};

#[derive(Debug, Default)]
pub struct CplOptions {
    pub corrections: Vec<String>,
    pub local_yaml: Option<String>,
    pub quiet: bool,
}

pub struct CplOutput {
    pub csv: String,
    pub report: String,
}

pub fn correct_position(
    pos_csv: &str,
    bom_csv: Option<&str>,
    database_csv: &str,
    local_yaml: Option<&str>,
    extra_csv: &[String],
) -> Result<CplOutput> {
    if is_empty(pos_csv) {
        return Ok(CplOutput {
            csv: "Designator,Mid X,Mid Y,Layer,Rotation\n".into(),
            report: String::new(),
        });
    }
    let mut db = Database::parse(database_csv).context("correction database")?;
    for extra in extra_csv {
        db.overlay(Database::parse(extra).context("corrections")?);
    }
    let local = match local_yaml {
        Some(text) => LocalDb::parse(text).context("local database")?,
        None => LocalDb::default(),
    };
    let pos = PosFile::parse(pos_csv).context("position file")?;
    let (bom, warnings) = match bom_csv {
        Some(text) => parse_bom(text).context("BOM")?,
        None => (HashMap::new(), Vec::new()),
    };
    let (cpl, mut report) = correct(&db, &local, &pos, &bom)?;
    report.warnings.extend(warnings);
    let wide = cpl.to_csv()?;
    Ok(CplOutput {
        csv: five_columns(&wide)?,
        report: report.to_string(),
    })
}

fn is_empty(text: &str) -> bool {
    text.lines().filter(|line| !line.trim().is_empty()).count() <= 1
}

pub fn five_columns(csv_text: &str) -> Result<String> {
    let mut reader = csv::ReaderBuilder::new()
        .flexible(true)
        .from_reader(csv_text.as_bytes());
    let headers = reader.headers().context("CPL header")?.clone();
    let index = |name: &str| {
        headers
            .iter()
            .position(|header| header.eq_ignore_ascii_case(name))
            .with_context(|| format!("missing column {name}"))
    };
    let designator = index("Designator")?;
    let x = index("Mid X")?;
    let y = index("Mid Y")?;
    let layer = index("Layer")?;
    let rotation = index("Rotation")?;
    let mut out = Vec::new();
    {
        let mut writer = csv::WriterBuilder::new()
            .terminator(csv::Terminator::CRLF)
            .from_writer(&mut out);
        writer.write_record(["Designator", "Mid X", "Mid Y", "Layer", "Rotation"])?;
        for record in reader.records() {
            let record = record.context("CPL row")?;
            let reference = record.get(designator).unwrap_or("").trim().to_string();
            if is_fiducial(&reference) {
                continue;
            }
            let layer_cell = record.get(layer).unwrap_or("").trim().to_ascii_lowercase();
            let side = match layer_cell.as_str() {
                "top" => "Top",
                "bottom" => "Bottom",
                _ => "",
            };
            let side = if side.is_empty() {
                record.get(layer).unwrap_or("").trim().to_string()
            } else {
                side.to_string()
            };
            let row = [
                reference,
                record.get(x).unwrap_or("").trim().to_string(),
                record.get(y).unwrap_or("").trim().to_string(),
                side,
                record.get(rotation).unwrap_or("").trim().to_string(),
            ];
            writer.write_record(&row)?;
        }
        writer.flush()?;
    }
    let mut text = String::from_utf8(out).context("writing CPL")?;
    text = text.replace("\r\n", "\n");
    Ok(text)
}

fn is_fiducial(reference: &str) -> bool {
    reference.len() > 2
        && reference.starts_with("FP")
        && reference[2..].chars().all(|c| c.is_ascii_digit())
}

#[allow(dead_code)]
pub fn unused_path(path: PathBuf) -> PathBuf {
    path
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drops_fiducials_and_normalizes_the_layer() {
        let csv = "\
Designator,Mid X,Mid Y,Layer,Rotation
R1,1,2,top,90
FP1,0,0,bottom,0
";
        let out = five_columns(csv).unwrap();
        assert!(out.contains("R1,1,2,Top,90"));
        assert!(!out.contains("FP1"));
        assert!(out.starts_with("Designator,Mid X,Mid Y,Layer,Rotation\n"));
    }
}
