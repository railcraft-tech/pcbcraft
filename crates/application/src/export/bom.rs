//! JLCPCB comment cleanup from `export-sbom.sh`.

use anyhow::{Context, Result};

pub fn rewrite_comments(csv_text: &str) -> Result<String> {
    let mut reader = csv::ReaderBuilder::new()
        .flexible(true)
        .from_reader(csv_text.as_bytes());
    let headers = reader.headers().context("BOM header")?.clone();
    let rows: Vec<csv::StringRecord> = reader
        .records()
        .collect::<Result<Vec<_>, _>>()
        .context("BOM")?;
    let comment = headers.iter().position(|name| name == "Comment");
    let footprint = headers.iter().position(|name| name == "Footprint");
    let mut out = Vec::new();
    {
        let mut writer = csv::WriterBuilder::new()
            .quote_style(csv::QuoteStyle::Always)
            .from_writer(&mut out);
        writer.write_record(&headers)?;
        for record in rows {
            let mut cells: Vec<String> = record.iter().map(str::to_string).collect();
            if let (Some(comment), Some(footprint)) = (comment, footprint) {
                if cells.get(comment).is_some_and(|value| value.contains(',')) {
                    let name = cells
                        .get(footprint)
                        .map(|value| {
                            value
                                .rsplit_once(':')
                                .map(|(_, name)| name)
                                .unwrap_or(value.as_str())
                                .to_string()
                        })
                        .unwrap_or_default();
                    cells[comment] = name;
                }
            }
            writer.write_record(&cells)?;
        }
        writer.flush()?;
    }
    String::from_utf8(out).context("writing BOM")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn comma_in_comment_becomes_the_footprint_name() {
        let csv = "Designator,Comment,Footprint,LCSC Part #\n\"R1\",\"1k, 1%\",\"Resistor_SMD:R_0805\",\"C1\"\n";
        let rewritten = rewrite_comments(csv).unwrap();
        assert!(rewritten.contains("\"R_0805\""));
        assert!(!rewritten.contains("1k, 1%"));
    }
}
