//! Electrical quantities written the way a catalog and a YAML spec write them.

use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dimension {
    Ohm,
    Farad,
    Henry,
    Volt,
    Watt,
    Ampere,
    Percent,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Quantity {
    pub si: f64,
    pub dimension: Dimension,
    pub raw: String,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum UnitError {
    #[error("unrecognized quantity '{0}'")]
    Unknown(String),
}

pub fn schematic_ohms(raw: &str) -> Option<f64> {
    if let Ok(quantity) = parse_quantity(raw) {
        return (quantity.dimension == Dimension::Ohm).then_some(quantity.si);
    }
    let text = raw.trim();
    if text.is_empty()
        || text.chars().filter(|c| *c == '.').count() > 1
        || !text.chars().all(|c| c.is_ascii_digit() || c == '.')
    {
        return None;
    }
    text.parse().ok()
}

pub fn parse_quantity(raw: &str) -> Result<Quantity, UnitError> {
    let trimmed = raw.trim().trim_start_matches(['±', '+']);
    let compact: String = trimmed
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>()
        .replace('Ω', "ohm")
        .replace('ω', "ohm")
        .replace('µ', "u")
        .replace('μ', "u");
    if compact.is_empty() {
        return Err(UnitError::Unknown(raw.into()));
    }
    let (dimension, number) = split_unit(&compact).ok_or_else(|| UnitError::Unknown(raw.into()))?;
    let si = parse_number(&number).ok_or_else(|| UnitError::Unknown(raw.into()))?;
    Ok(Quantity {
        si,
        dimension,
        raw: raw.trim().to_string(),
    })
}

fn split_unit(text: &str) -> Option<(Dimension, String)> {
    let lower = text.to_ascii_lowercase();
    if let Some(body) = lower.strip_suffix('%') {
        return Some((Dimension::Percent, body.to_string()));
    }
    if lower.ends_with('w') && !lower.contains("ohm") {
        return Some((Dimension::Watt, power_body(&lower)?));
    }
    if let Some(body) = lower.strip_suffix("henry") {
        return Some((Dimension::Henry, body.to_string()));
    }
    if let Some(body) = lower.strip_suffix('h') {
        if !body.ends_with("om") {
            return Some((Dimension::Henry, body.to_string()));
        }
    }
    if let Some(body) = lower.strip_suffix("farad") {
        return Some((Dimension::Farad, body.to_string()));
    }
    if let Some(body) = lower.strip_suffix('f') {
        return Some((Dimension::Farad, body.to_string()));
    }
    if let Some(body) = lower.strip_suffix("ohm") {
        return Some((Dimension::Ohm, preserve_mega(text, body)));
    }
    if let Some(body) = lower.strip_suffix('v') {
        return Some((Dimension::Volt, body.to_string()));
    }
    if let Some(body) = lower.strip_suffix('a') {
        if !body.is_empty()
            && body
                .chars()
                .all(|c| c.is_ascii_digit() || c == '.' || is_prefix(c))
        {
            return Some((Dimension::Ampere, body.to_string()));
        }
    }
    if let Some(body) = lower.strip_suffix('r') {
        return Some((Dimension::Ohm, preserve_mega(text, body)));
    }
    if lower.ends_with(['k', 'm']) && lower.chars().any(|c| c.is_ascii_digit()) {
        return Some((Dimension::Ohm, preserve_mega(text, &lower)));
    }
    if lower.ends_with(['p', 'n', 'u']) && lower.chars().any(|c| c.is_ascii_digit()) {
        return Some((Dimension::Farad, lower));
    }
    None
}

/// `1M` is mega. A lower-case `m` stays milli.
fn preserve_mega(original: &str, lowered_body: &str) -> String {
    let orig_compact: String = original.chars().filter(|c| !c.is_whitespace()).collect();
    if orig_compact.ends_with('M') || orig_compact.ends_with("Mohm") || orig_compact.ends_with("MΩ")
    {
        let out = lowered_body.to_string();
        if let Some(stripped) = out.strip_suffix('m') {
            return format!("{stripped}M");
        }
    }
    lowered_body.to_string()
}

fn is_prefix(c: char) -> bool {
    matches!(c, 'p' | 'n' | 'u' | 'm' | 'k' | 'g')
}

fn power_body(text: &str) -> Option<String> {
    let body = text.strip_suffix('w')?;
    if let Some((num, den)) = body.split_once('/') {
        let num: f64 = num.parse().ok()?;
        let den: f64 = den.parse().ok()?;
        if den == 0.0 {
            return None;
        }
        return Some(format!("{}", num / den));
    }
    Some(body.to_string())
}

fn parse_number(text: &str) -> Option<f64> {
    let (digits, mult) = split_prefix(text)?;
    let value: f64 = digits.parse().ok()?;
    Some(value * mult)
}

fn split_prefix(text: &str) -> Option<(&str, f64)> {
    if text.is_empty() {
        return None;
    }
    let last = text.chars().last()?;
    let mult = match last {
        'p' => 1e-12,
        'n' => 1e-9,
        'u' => 1e-6,
        'm' => 1e-3,
        'k' => 1e3,
        'M' => 1e6,
        'g' => 1e9,
        _ => return Some((text, 1.0)),
    };
    let body = &text[..text.len() - last.len_utf8()];
    if body.is_empty() {
        return None;
    }
    Some((body, mult))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_catalog_spellings() {
        assert!((parse_quantity("10k").unwrap().si - 10_000.0).abs() < 1e-6);
        assert_eq!(parse_quantity("10k").unwrap().dimension, Dimension::Ohm);
        assert!((parse_quantity("10 kΩ").unwrap().si - 10_000.0).abs() < 1e-6);
        assert!((parse_quantity("100nF").unwrap().si - 100e-9).abs() < 1e-18);
        assert!((parse_quantity("1%").unwrap().si - 1.0).abs() < 1e-9);
        assert!((parse_quantity("±1%").unwrap().si - 1.0).abs() < 1e-9);
        assert!((parse_quantity("1/16W").unwrap().si - 0.0625).abs() < 1e-9);
        assert!((parse_quantity("16V").unwrap().si - 16.0).abs() < 1e-9);
        assert_eq!(parse_quantity("16V").unwrap().dimension, Dimension::Volt);
        assert_eq!(schematic_ohms("22"), Some(22.0));
        assert_eq!(schematic_ohms("10k"), Some(10_000.0));
        assert_eq!(schematic_ohms("100nF"), None);
    }
}
