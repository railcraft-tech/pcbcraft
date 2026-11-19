//! One subtest per LCSC or PCBWay part used on the schematic.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json::Value;

use super::scheduler::TestContext;
use super::validator::{Job, Line, ValidatorInterface};
use crate::ports::{PartStock, UsedPart, UsedParts};

const DEFAULT_MINIMUM: u64 = 100;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Case {
    pub name: String,
    pub subtest: bool,
    pub passed: bool,
    pub details: Vec<String>,
}

pub struct PartAvailability;

#[derive(Deserialize)]
struct Spec {
    project: String,
    #[serde(default = "default_minimum", rename = "defaultMinimum")]
    default_minimum: u64,
    /// Optional per-part stock floor. The key is the LCSC, PCBWay, or other catalog number.
    #[serde(default)]
    part: BTreeMap<String, u64>,
}

fn default_minimum() -> u64 {
    DEFAULT_MINIMUM
}

impl PartAvailability {
    pub fn validate(
        &self,
        spec: &Value,
        parts: &dyn UsedParts,
        stock: &dyn PartStock,
    ) -> Result<Vec<Case>> {
        Ok(self
            .prepare(spec, parts)?
            .into_iter()
            .map(|check| check.finish(stock))
            .collect())
    }

    /// Part list for one file, before any stock lookup.
    ///
    /// The scheduler prints the suite header from this list, then calls
    /// [`Check::finish`] one part at a time so each line can appear immediately.
    pub fn prepare(&self, spec: &Value, parts: &dyn UsedParts) -> Result<Vec<Check>> {
        let spec: Spec = serde_json::from_value(spec.clone()).context("spec PartAvailability")?;
        let mut used = parts.list(std::path::Path::new(&spec.project))?;
        used.sort_by(|left, right| {
            left.part_id
                .cmp(&right.part_id)
                .then(left.supplier.as_str().cmp(right.supplier.as_str()))
        });
        used.dedup();
        if used.is_empty() {
            return Ok(vec![Check::Done(Case {
                name: String::new(),
                subtest: false,
                passed: false,
                details: vec!["no LCSC or PCBWay part numbers on the schematic".into()],
            })]);
        }
        let names = display_names(&used);
        Ok(used
            .into_iter()
            .zip(names)
            .map(|(part, name)| {
                let minimum = spec
                    .part
                    .get(&part.part_id)
                    .copied()
                    .unwrap_or(spec.default_minimum);
                Check::Stock {
                    name,
                    part,
                    minimum,
                }
            })
            .collect())
    }
}

impl ValidatorInterface for PartAvailability {
    fn type_name(&self) -> &'static str {
        "PartAvailability"
    }

    fn schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "additionalProperties": false,
            "required": ["project"],
            "properties": {
                "project": { "type": "string", "minLength": 1 },
                "defaultMinimum": { "type": "integer", "minimum": 0 },
                "part": {
                    "type": "object",
                    "additionalProperties": { "type": "integer", "minimum": 0 }
                }
            }
        })
    }

    fn run(&self, file_name: &str, spec: &Value, ctx: &TestContext<'_>) -> Result<Vec<Job>> {
        let checks = self.prepare(spec, ctx.used_parts)?;
        let single_plain = matches!(checks.as_slice(), [Check::Done(case)] if !case.subtest);
        let file_name = file_name.to_string();
        Ok(checks
            .into_iter()
            .map(|check| {
                let label = match &check {
                    Check::Done(case) if case.name.is_empty() => file_name.clone(),
                    Check::Done(case) => case.name.clone(),
                    Check::Stock { name, .. } => name.clone(),
                };
                let owned_name = file_name.clone();
                Job {
                    label,
                    subtest: !single_plain,
                    run: Box::new(move |ctx| {
                        let case = check.finish(ctx.stock);
                        let name = if case.name.is_empty() {
                            owned_name
                        } else {
                            case.name
                        };
                        Ok(Line {
                            name,
                            subtest: !single_plain,
                            status: if case.passed {
                                crate::ports::TestStatus::Passed
                            } else {
                                crate::ports::TestStatus::Failed
                            },
                            details: case.details,
                            assertions: 1,
                        })
                    }),
                }
            })
            .collect())
    }
}

/// One availability line, either already decided or waiting on a stock lookup.
pub enum Check {
    Done(Case),
    Stock {
        name: String,
        part: UsedPart,
        minimum: u64,
    },
}

impl Check {
    pub fn finish(self, stock: &dyn PartStock) -> Case {
        match self {
            Check::Done(case) => case,
            Check::Stock {
                name,
                part,
                minimum,
            } => judge(&name, &part, minimum, stock),
        }
    }
}

fn display_names(parts: &[UsedPart]) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut duplicated = BTreeSet::new();
    for part in parts {
        if !seen.insert(part.part_id.clone()) {
            duplicated.insert(part.part_id.clone());
        }
    }
    parts
        .iter()
        .map(|part| {
            if duplicated.contains(&part.part_id) {
                format!("{}:{}", part.supplier, part.part_id)
            } else {
                part.part_id.clone()
            }
        })
        .collect()
}

fn judge(name: &str, part: &UsedPart, minimum: u64, stock: &dyn PartStock) -> Case {
    match stock.lookup(part.supplier, &part.part_id) {
        Ok(Some(quantity)) if quantity >= minimum => Case {
            name: name.to_string(),
            subtest: true,
            passed: true,
            details: Vec::new(),
        },
        Ok(Some(quantity)) => Case {
            name: name.to_string(),
            subtest: true,
            passed: false,
            details: vec![format!("stock {quantity} is below {minimum}")],
        },
        Ok(None) => Case {
            name: name.to_string(),
            subtest: true,
            passed: false,
            details: vec![format!("no stock reported; minimum is {minimum}")],
        },
        Err(err) => Case {
            name: name.to_string(),
            subtest: true,
            passed: false,
            details: vec![err.to_string()],
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pcbcraft_domain::parts::Supplier;
    use std::path::Path;

    struct Listed(Vec<UsedPart>);

    impl UsedParts for Listed {
        fn list(&self, _: &Path) -> Result<Vec<UsedPart>> {
            Ok(self.0.clone())
        }
    }

    struct Stocks;

    impl PartStock for Stocks {
        fn lookup(&self, _: Supplier, part_id: &str) -> Result<Option<u64>> {
            Ok(match part_id {
                "C1" => Some(100),
                "C2" => Some(12),
                _ => None,
            })
        }
    }

    #[test]
    fn each_part_is_its_own_subtest() {
        let spec = serde_json::json!({"project": "board.kicad_pro"});
        let cases = PartAvailability
            .validate(
                &spec,
                &Listed(vec![
                    UsedPart {
                        supplier: Supplier::Lcsc,
                        part_id: "C2".into(),
                    },
                    UsedPart {
                        supplier: Supplier::Lcsc,
                        part_id: "C1".into(),
                    },
                ]),
                &Stocks,
            )
            .unwrap();
        assert_eq!(cases[0].name, "C1");
        assert!(cases[0].passed);
        assert_eq!(cases[1].name, "C2");
        assert!(!cases[1].passed);
        assert!(cases[1].details[0].contains("12"));
    }

    #[test]
    fn a_missing_threshold_is_one_hundred() {
        let spec = serde_json::json!({"project": "board.kicad_pro", "defaultMinimum": 10});
        let cases = PartAvailability
            .validate(
                &spec,
                &Listed(vec![UsedPart {
                    supplier: Supplier::Lcsc,
                    part_id: "C2".into(),
                }]),
                &Stocks,
            )
            .unwrap();
        assert!(cases[0].passed);
    }

    #[test]
    fn a_part_entry_overrides_the_default_minimum() {
        let spec = serde_json::json!({
            "project": "board.kicad_pro",
            "defaultMinimum": 100,
            "part": { "C2": 200, "C1": 50 }
        });
        let cases = PartAvailability
            .validate(
                &spec,
                &Listed(vec![
                    UsedPart {
                        supplier: Supplier::Lcsc,
                        part_id: "C1".into(),
                    },
                    UsedPart {
                        supplier: Supplier::Lcsc,
                        part_id: "C2".into(),
                    },
                ]),
                &Stocks,
            )
            .unwrap();
        assert!(cases[0].passed);
        assert!(!cases[1].passed);
        assert!(cases[1].details[0].contains("200"));
    }
}
