//! Korekta pliku pozycji KiCad do montażu JLCPCB.
//!
//! Reguły pochodzą z tej samej tabeli, którą pobiera wtyczka
//! kicad-jlcpcb-tools: [`CORRECTIONS_URL`]. Kąt na spodzie jest odbijany,
//! a przesunięcie jest obracane w układzie footprintu — tak jak przy zapisie
//! CPL w `fabrication.py` tej wtyczki.

#![forbid(unsafe_code)]

mod correct;
mod db;
mod local;
mod placement;
mod table;

pub use correct::{correct, Report};
pub use db::{Database, Hit, MatchField, CORRECTIONS_URL};
pub use local::LocalDb;
pub use table::{parse_bom, Cpl, PosFile};

/// Złożenie nagłówka CSV do porównań: małe litery, `_` i `-` jako spacja.
pub(crate) fn header_key(name: &str) -> String {
    let mut out = String::new();
    let mut spaced = false;
    for c in name.trim().chars() {
        if c.is_whitespace() || c == '_' || c == '-' {
            spaced = !out.is_empty();
            continue;
        }
        if c == '#' {
            continue;
        }
        if spaced {
            out.push(' ');
            spaced = false;
        }
        for lower in c.to_lowercase() {
            out.push(lower);
        }
    }
    out
}

pub(crate) fn strip_bom(text: &str) -> &str {
    text.strip_prefix('\u{feff}').unwrap_or(text)
}

/// Pierwszy niepusty wiersz. Średnik wygrywa, gdy jest go więcej niż przecinków
/// (arkusz zapisany w europejskim Excelu).
pub(crate) fn delimiter(text: &str) -> u8 {
    let line = text
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("");
    if line.matches(';').count() > line.matches(',').count() {
        b';'
    } else {
        b','
    }
}

#[cfg(test)]
mod tests {
    use super::header_key;

    #[test]
    fn header_keys_match_the_plugin_aliases() {
        assert_eq!(header_key("Offset X"), "offset x");
        assert_eq!(header_key("offset_x"), "offset x");
        assert_eq!(header_key("LCSC Part #"), "lcsc part");
        assert_eq!(header_key("PosX"), "posx");
        assert_eq!(header_key("Mid X"), "mid x");
        assert_eq!(header_key("Footprint pattern"), "footprint pattern");
        assert_eq!(header_key("  Rotation "), "rotation");
    }
}
