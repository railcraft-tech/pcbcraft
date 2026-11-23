//! PyMuPDF, run as a separate program. The library is AGPL-3.0 or commercial.

use std::fs;
use std::path::Path;
use std::process::Command;

use anyhow::{bail, Context, Result};
use pcbcraft_application::parts::ConvertedPdf;

const SCRIPT: &str = include_str!("../../../tools/pdf2md.py");
const INSTALL: &str =
    "pip install 'pymupdf==2.1' (or, with PyMuPDF 1.x already installed, pip install 'pymupdf4llm<2')";

#[derive(Debug, Default, Clone, Copy)]
pub struct Pymupdf;

impl pcbcraft_application::parts::PdfToMarkdown for Pymupdf {
    fn convert(&self, dir: &Path) -> Result<ConvertedPdf> {
        let script = dir.join("pdf2md.py");
        fs::write(&script, SCRIPT)
            .with_context(|| format!("could not write {}", script.display()))?;
        let output = Command::new("python3")
            .arg(&script)
            .current_dir(dir)
            .env("PYTHONDONTWRITEBYTECODE", "1")
            .output()
            .map_err(|err| {
                anyhow::anyhow!("cannot run python3: {err}. Install PyMuPDF with: {INSTALL}")
            })?;
        let _ = fs::remove_file(&script);
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stderr = stderr.trim();
            if stderr.is_empty() {
                bail!("PDF conversion failed. Install PyMuPDF with: {INSTALL}");
            }
            bail!("{stderr}");
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        let (pages, markdown) = split_pages(&stdout)?;
        let warning = String::from_utf8_lossy(&output.stderr);
        let warning = warning
            .lines()
            .find(|line| line.contains("no text layer"))
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(str::to_string);
        Ok(ConvertedPdf {
            markdown,
            pages,
            warning,
        })
    }
}

fn split_pages(text: &str) -> Result<(u32, String)> {
    let mut offset = 0;
    for line in text.split_inclusive('\n').take(30) {
        let trimmed = line.trim_end_matches(['\n', '\r']);
        if let Some(number) = trimmed.strip_prefix("pages ") {
            if let Ok(pages) = number.trim().parse() {
                return Ok((pages, text[offset + line.len()..].to_string()));
            }
        }
        offset += line.len();
    }
    anyhow::bail!("PDF conversion returned no page count")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_page_count_is_the_first_line() {
        let (pages, body) = split_pages("pages 43\n# Title\n").unwrap();
        assert_eq!(pages, 43);
        assert_eq!(body, "# Title\n");
        let (pages, body) = split_pages("Warning - arguments ignored.\npages 9\nbody\n").unwrap();
        assert_eq!(pages, 9);
        assert_eq!(body, "body\n");
        assert!(split_pages("not a header\n").is_err());
    }
}
