//! LCSC query and the local filter that decides a real alternative.

use std::collections::BTreeMap;

use super::offer::PartRecord;
use super::units::{parse_quantity, Dimension, Quantity};

#[derive(Debug, Clone, PartialEq)]
pub enum Constraint {
    Range { min: Quantity, max: Quantity },
    OneOf(Vec<String>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct LcscQuery {
    pub category: String,
    pub package: Vec<String>,
    pub constraints: BTreeMap<String, Constraint>,
}

pub fn keyword(query: &LcscQuery) -> String {
    let mut parts = Vec::new();
    if !query.category.is_empty() {
        parts.push(query.category.clone());
    }
    if let Some(package) = query.package.first() {
        parts.push(package.clone());
    }
    for key in ["resistance", "capacitance", "inductance"] {
        if let Some(Constraint::Range { min, max }) = query.constraints.get(key) {
            let mid = (min.si + max.si) / 2.0;
            parts.push(format_eng(mid, min.dimension));
            break;
        }
    }
    parts.join(" ")
}

pub fn matches(record: &PartRecord, query: &LcscQuery) -> bool {
    if !query.package.is_empty()
        && !query.package.iter().any(|package| {
            package_key(&record.package).contains(&package_key(package))
                || package_key(package).contains(&package_key(&record.package))
        })
    {
        return false;
    }
    query.constraints.iter().all(|(key, constraint)| {
        let Some(param) = record.parameters.iter().find(|param| {
            aliases(key)
                .iter()
                .any(|alias| param_key(&param.name) == *alias)
                || param_key(&param.name) == param_key(key)
        }) else {
            return false;
        };
        match constraint {
            Constraint::OneOf(accepted) => accepted.iter().any(|item| {
                param_key(&param.value) == param_key(item)
                    || param_key(&param.value).contains(&param_key(item))
            }),
            Constraint::Range { min, max } => {
                let Some(si) = param
                    .si
                    .or_else(|| parse_quantity(&param.value).ok().map(|q| q.si))
                else {
                    return false;
                };
                si + 1e-12 >= min.si.min(max.si) && si - 1e-12 <= min.si.max(max.si)
            }
        }
    })
}

fn package_key(text: &str) -> String {
    text.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .flat_map(|c| c.to_lowercase())
        .collect()
}

fn param_key(text: &str) -> String {
    text.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .flat_map(|c| c.to_lowercase())
        .collect()
}

/// Catalog attribute parsed back into a quantity, matched by the same aliases as search.
pub fn tolerance_window(
    record: &PartRecord,
    part: Dimension,
) -> Option<crate::electrical::Tolerance> {
    let param = record.parameters.iter().find(|param| {
        aliases("tolerance")
            .iter()
            .any(|alias| param_key(&param.name) == *alias)
            || param_key(&param.name) == param_key("tolerance")
    })?;
    let quantity = parse_quantity(&param.value).ok()?;
    match quantity.dimension {
        Dimension::Percent => Some(crate::electrical::Tolerance::Relative(quantity.si / 100.0)),
        dimension if dimension == part => Some(crate::electrical::Tolerance::Absolute(quantity.si)),
        _ => None,
    }
}

/// Catalog attribute parsed back into a quantity, matched by the same aliases as search.
pub fn attribute_quantity(record: &PartRecord, key: &str) -> Option<Quantity> {
    let expected = match key {
        "resistance" => Dimension::Ohm,
        "capacitance" => Dimension::Farad,
        "inductance" => Dimension::Henry,
        "tolerance" => Dimension::Percent,
        "power" => Dimension::Watt,
        "voltage" => Dimension::Volt,
        "current" => Dimension::Ampere,
        _ => return None,
    };
    let param = record.parameters.iter().find(|param| {
        aliases(key)
            .iter()
            .any(|alias| param_key(&param.name) == *alias)
            || param_key(&param.name) == param_key(key)
    })?;
    let quantity = parse_quantity(&param.value).ok()?;
    if quantity.dimension != expected {
        return None;
    }
    Some(quantity)
}

fn aliases(key: &str) -> &'static [&'static str] {
    match key {
        "resistance" => &["resistance"],
        "capacitance" => &["capacitance"],
        "inductance" => &["inductance"],
        "tolerance" => &["tolerance"],
        "power" => &["power", "powerrating"],
        "voltage" => &["voltage", "voltagerating", "ratedvoltage"],
        "current" => &["current", "currentrating"],
        "dielectric" => &["dielectric", "temperaturecoefficient"],
        _ => &[],
    }
}

fn format_eng(si: f64, dimension: Dimension) -> String {
    let (scale, suffix) = match dimension {
        Dimension::Ohm | Dimension::Volt | Dimension::Watt | Dimension::Ampere => {
            if si >= 1e6 {
                (1e6, "M")
            } else if si >= 1e3 {
                (1e3, "k")
            } else {
                (1.0, "")
            }
        }
        Dimension::Farad | Dimension::Henry => {
            if si >= 1e-3 {
                (1e-3, "m")
            } else if si >= 1e-6 {
                (1e-6, "u")
            } else if si >= 1e-9 {
                (1e-9, "n")
            } else {
                (1e-12, "p")
            }
        }
        Dimension::Percent => (1.0, ""),
    };
    let unit = match dimension {
        Dimension::Ohm => "ohm",
        Dimension::Farad => "F",
        Dimension::Henry => "H",
        Dimension::Volt => "V",
        Dimension::Watt => "W",
        Dimension::Ampere => "A",
        Dimension::Percent => "%",
    };
    format!("{}{suffix}{unit}", trim_num(si / scale))
}

fn trim_num(value: f64) -> String {
    let text = format!("{value:.4}");
    text.trim_end_matches('0').trim_end_matches('.').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parts::{parse_quantity, ElecParam, PartRecord, Supplier};

    fn record(package: &str, resistance: &str) -> PartRecord {
        PartRecord {
            supplier: Supplier::Lcsc,
            part_id: "C1".into(),
            mpn: "RC0805".into(),
            manufacturer: "Yageo".into(),
            package: package.into(),
            category: "Resistors".into(),
            description: String::new(),
            stock: Some(100),
            price: Some(0.01),
            library: Some("basic".into()),
            parameters: vec![ElecParam {
                name: "Resistance".into(),
                value: resistance.into(),
                si: parse_quantity(resistance).ok().map(|q| q.si),
            }],
        }
    }

    #[test]
    fn keyword_uses_the_middle_of_the_range() {
        let query = LcscQuery {
            category: "resistor".into(),
            package: vec!["0805".into()],
            constraints: BTreeMap::from([(
                "resistance".into(),
                Constraint::Range {
                    min: parse_quantity("9k").unwrap(),
                    max: parse_quantity("11k").unwrap(),
                },
            )]),
        };
        assert_eq!(keyword(&query), "resistor 0805 10kohm");
    }

    #[test]
    fn a_part_outside_the_range_or_package_is_rejected() {
        let query = LcscQuery {
            category: "resistor".into(),
            package: vec!["0805".into()],
            constraints: BTreeMap::from([(
                "resistance".into(),
                Constraint::Range {
                    min: parse_quantity("9.9k").unwrap(),
                    max: parse_quantity("10.1k").unwrap(),
                },
            )]),
        };
        assert!(matches(&record("0805", "10k"), &query));
        assert!(!matches(&record("0603", "10k"), &query));
        assert!(!matches(&record("0805", "100k"), &query));
    }
}
