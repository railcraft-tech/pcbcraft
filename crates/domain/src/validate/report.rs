use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Symbol {
    pub reference: String,
    pub value: String,
    pub footprint: String,
    /// `Device:R`
    pub library: String,
    pub properties: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PinRef {
    pub reference: String,
    pub pin: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Net {
    pub name: String,
    pub pins: Vec<PinRef>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warning,
    Exclusion,
}

impl Severity {
    pub fn parse(text: &str) -> Self {
        match text.to_ascii_lowercase().as_str() {
            "error" => Self::Error,
            "warning" => Self::Warning,
            "exclusion" | "ignore" | "excluded" => Self::Exclusion,
            _ => Self::Error,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ViolationItem {
    pub description: String,
    pub uuid: Option<String>,
    pub x_mm: Option<f64>,
    pub y_mm: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Violation {
    pub check: String,
    pub severity: Severity,
    pub kind: String,
    pub description: String,
    pub items: Vec<ViolationItem>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Design {
    pub symbols: Vec<Symbol>,
    pub nets: Vec<Net>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Report {
    pub symbols: Vec<Symbol>,
    pub nets: Vec<Net>,
    pub violations: Vec<Violation>,
}

impl Report {
    pub fn from_design(design: Design, violations: Vec<Violation>) -> Self {
        Self {
            symbols: design.symbols,
            nets: design.nets,
            violations,
        }
    }

    pub fn has_error(&self) -> bool {
        self.violations
            .iter()
            .any(|item| item.severity == Severity::Error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_warning_does_not_fail_the_report() {
        let report = Report {
            symbols: Vec::new(),
            nets: Vec::new(),
            violations: vec![Violation {
                check: "erc".into(),
                severity: Severity::parse("warning"),
                kind: "label".into(),
                description: "loose".into(),
                items: Vec::new(),
            }],
        };
        assert!(!report.has_error());
        assert_eq!(Severity::parse("excluded"), Severity::Exclusion);
    }
}
