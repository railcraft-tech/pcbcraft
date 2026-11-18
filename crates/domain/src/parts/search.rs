//! YAML spec. `supplier` chooses the parameter struct, and the two structs do not share keys.

use std::collections::BTreeMap;

use serde_yaml::Value;
use thiserror::Error;

use super::lcsc::{Constraint, LcscQuery};
use super::offer::PartRecord;
use super::pcbway::PcbwayQuery;
use super::supplier::Supplier;
use super::units::{parse_quantity, Quantity};

#[derive(Debug, Clone, PartialEq)]
pub struct SpecFile {
    pub items: Vec<Item>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub name: String,
    pub supplier: Supplier,
    pub query: Query,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Query {
    Lcsc(LcscQuery),
    Pcbway(PcbwayQuery),
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum SpecError {
    #[error("{0}")]
    Message(String),
}

pub trait SupplierCatalog {
    type Query;
    fn supplier(&self) -> Supplier;
    fn fetch(&self, part_ids: &[String]) -> anyhow::Result<Vec<PartRecord>>;
    fn search(&self, query: &Self::Query) -> anyhow::Result<Vec<PartRecord>>;
}

pub fn parse_spec(text: &str) -> Result<SpecFile, SpecError> {
    let value: Value =
        serde_yaml::from_str(text).map_err(|err| SpecError::Message(format!("YAML: {err}")))?;
    let root = value
        .as_mapping()
        .ok_or_else(|| SpecError::Message("the spec must be a mapping".into()))?;
    let items = root
        .get(Value::String("items".into()))
        .and_then(Value::as_sequence)
        .ok_or_else(|| SpecError::Message("missing items list".into()))?;
    let mut parsed = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        parsed.push(
            parse_item(item)
                .map_err(|err| SpecError::Message(format!("item {}: {err}", index + 1)))?,
        );
    }
    Ok(SpecFile { items: parsed })
}

fn parse_item(value: &Value) -> Result<Item, SpecError> {
    let map = value
        .as_mapping()
        .ok_or_else(|| SpecError::Message("an item must be a mapping".into()))?;
    let name = string_field(map, "name")?;
    let supplier = match string_field(map, "supplier")?.as_str() {
        "lcsc" => Supplier::Lcsc,
        "pcbway" => Supplier::Pcbway,
        other => return Err(SpecError::Message(format!("unknown supplier '{other}'"))),
    };
    let params = map
        .get(Value::String("parameters".into()))
        .ok_or_else(|| SpecError::Message("missing parameters".into()))?;
    let query = match supplier {
        Supplier::Lcsc => Query::Lcsc(parse_lcsc(params)?),
        Supplier::Pcbway => Query::Pcbway(parse_pcbway(params)?),
    };
    Ok(Item {
        name,
        supplier,
        query,
    })
}

fn parse_lcsc(value: &Value) -> Result<LcscQuery, SpecError> {
    let map = value
        .as_mapping()
        .ok_or_else(|| SpecError::Message("LCSC parameters must be a mapping".into()))?;
    let mut category = String::new();
    let mut package = Vec::new();
    let mut constraints = BTreeMap::new();
    for (key, raw) in map {
        let key = key
            .as_str()
            .ok_or_else(|| SpecError::Message("LCSC parameter key is not a string".into()))?
            .to_string();
        match key.as_str() {
            "category" => category = yaml_string(raw)?,
            "package" => package = string_or_list(raw)?,
            "mpn" | "manufacturer" | "description" => {
                return Err(SpecError::Message(format!(
                    "key '{key}' belongs to pcbway, but this item's supplier is lcsc"
                )));
            }
            _ => {
                constraints.insert(key, parse_constraint(raw)?);
            }
        }
    }
    if category.is_empty() {
        return Err(SpecError::Message(
            "LCSC requires category inside parameters".into(),
        ));
    }
    Ok(LcscQuery {
        category,
        package,
        constraints,
    })
}

fn parse_pcbway(value: &Value) -> Result<PcbwayQuery, SpecError> {
    let map = value
        .as_mapping()
        .ok_or_else(|| SpecError::Message("PCBWay parameters must be a mapping".into()))?;
    let mut mpn = Vec::new();
    let mut manufacturer = Vec::new();
    let mut package = Vec::new();
    let mut description = Vec::new();
    for (key, raw) in map {
        let key = key
            .as_str()
            .ok_or_else(|| SpecError::Message("PCBWay parameter key is not a string".into()))?;
        match key {
            "mpn" => mpn = string_list_only(raw, "mpn")?,
            "manufacturer" => manufacturer = string_list_only(raw, "manufacturer")?,
            "package" => package = string_list_only(raw, "package")?,
            "description" => description = string_list_only(raw, "description")?,
            other => {
                return Err(SpecError::Message(format!(
                    "key '{other}' does not belong to pcbway"
                )));
            }
        }
    }
    if mpn.is_empty() {
        return Err(SpecError::Message(
            "PCBWay requires an mpn list with at least one number".into(),
        ));
    }
    Ok(PcbwayQuery {
        mpn,
        manufacturer,
        package,
        description,
    })
}

fn parse_constraint(value: &Value) -> Result<Constraint, SpecError> {
    match value {
        Value::Sequence(items) => {
            let mut values = Vec::new();
            for item in items {
                values.push(yaml_string(item)?);
            }
            if values.is_empty() {
                return Err(SpecError::Message("value list is empty".into()));
            }
            Ok(Constraint::OneOf(values))
        }
        Value::Mapping(map) => {
            if map.len() != 2 || !map.contains_key("min") || !map.contains_key("max") {
                return Err(SpecError::Message("a range needs both min and max".into()));
            }
            let min = quantity_text(map.get("min").unwrap())?;
            let max = quantity_text(map.get("max").unwrap())?;
            if min.dimension != max.dimension {
                return Err(SpecError::Message(
                    "min and max must be the same quantity".into(),
                ));
            }
            Ok(Constraint::Range { min, max })
        }
        Value::Number(_) => Err(SpecError::Message(
            "a bare number is not a range or a list".into(),
        )),
        _ => Err(SpecError::Message(
            "an electrical parameter must be a range or a list".into(),
        )),
    }
}

fn quantity_text(value: &Value) -> Result<Quantity, SpecError> {
    let text = yaml_string(value)?;
    parse_quantity(&text).map_err(|err| SpecError::Message(err.to_string()))
}

fn yaml_string(value: &Value) -> Result<String, SpecError> {
    value
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| SpecError::Message("expected a string, not a number".into()))
}

fn string_or_list(value: &Value) -> Result<Vec<String>, SpecError> {
    match value {
        Value::String(text) => Ok(vec![text.clone()]),
        Value::Sequence(items) => items.iter().map(yaml_string).collect(),
        _ => Err(SpecError::Message("expected a string or a list".into())),
    }
}

fn string_list_only(value: &Value, key: &str) -> Result<Vec<String>, SpecError> {
    match value {
        Value::Sequence(items) => {
            let values: Result<Vec<_>, _> = items.iter().map(yaml_string).collect();
            let values = values?;
            if values.is_empty() {
                return Err(SpecError::Message(format!("{key} list is empty")));
            }
            Ok(values)
        }
        Value::Mapping(_) => Err(SpecError::Message(format!(
            "{key} for pcbway is a list of strings, not a range"
        ))),
        _ => Err(SpecError::Message(format!(
            "{key} for pcbway is a list of strings"
        ))),
    }
}

fn string_field(map: &serde_yaml::Mapping, key: &str) -> Result<String, SpecError> {
    map.get(Value::String(key.into()))
        .ok_or_else(|| SpecError::Message(format!("missing {key}")))
        .and_then(yaml_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_a_foreign_key_and_a_bare_number() {
        let foreign = r#"
items:
  - name: R1
    supplier: lcsc
    parameters:
      category: resistor
      mpn: ["X"]
"#;
        assert!(parse_spec(foreign)
            .unwrap_err()
            .to_string()
            .contains("pcbway"));
        let bare = r#"
items:
  - name: R1
    supplier: lcsc
    parameters:
      category: resistor
      resistance: 10000
"#;
        assert!(parse_spec(bare)
            .unwrap_err()
            .to_string()
            .contains("bare number"));
    }

    #[test]
    fn reads_both_suppliers() {
        let text = r#"
items:
  - name: R12
    supplier: lcsc
    parameters:
      category: resistor
      package: "0805"
      resistance: { min: "9.9k", max: "10.1k" }
  - name: U1
    supplier: pcbway
    parameters:
      mpn: ["STM32F103C8T6"]
"#;
        let spec = parse_spec(text).unwrap();
        assert_eq!(spec.items.len(), 2);
        assert_eq!(spec.items[1].supplier, Supplier::Pcbway);
    }
}
