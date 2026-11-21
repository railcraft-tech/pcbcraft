//! Product attributes for a catalog search.
//!
//! A JSON object maps a name to a string or a list of strings. `package` and
//! `manufacturer` select those catalog fields. Every other name is sent as
//! written, because the catalog matches the spelling it prints.

use std::collections::BTreeMap;

use serde_json::Value;
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ProductAttributes {
    pub package: Vec<String>,
    pub manufacturer: Vec<String>,
    pub attributes: BTreeMap<String, Vec<String>>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum AttrError {
    #[error("{0}")]
    Message(String),
}

pub fn parse_product_attributes_text(text: &str) -> Result<ProductAttributes, AttrError> {
    let value: Value = serde_json::from_str(text)
        .map_err(|err| AttrError::Message(format!("product attributes: {err}")))?;
    parse_product_attributes(&value)
}

pub fn parse_product_attributes(value: &Value) -> Result<ProductAttributes, AttrError> {
    let Some(map) = value.as_object() else {
        return Err(AttrError::Message(
            "product attributes must be a JSON object".into(),
        ));
    };
    let mut attributes = ProductAttributes::default();
    for (key, raw) in map {
        let values = string_values(raw)
            .map_err(|err| AttrError::Message(format!("attribute '{key}': {err}")))?;
        attributes.add(key, values)?;
    }
    Ok(attributes)
}

/// One `-p key=value` entry. The same key again adds another accepted value.
pub fn add_param(attributes: &mut ProductAttributes, text: &str) -> Result<(), AttrError> {
    let Some((key, value)) = text.split_once('=') else {
        return Err(AttrError::Message(format!(
            "-p expects key=value, got '{text}'"
        )));
    };
    attributes.add(key, vec![value.to_string()])
}

impl ProductAttributes {
    pub fn add(&mut self, key: &str, values: Vec<String>) -> Result<(), AttrError> {
        let key = key.trim();
        if key.is_empty() {
            return Err(AttrError::Message("attribute key is empty".into()));
        }
        let mut cleaned = Vec::with_capacity(values.len());
        for value in values {
            let value = value.trim();
            if value.is_empty() {
                return Err(AttrError::Message(format!(
                    "attribute '{key}' has an empty value"
                )));
            }
            cleaned.push(value.to_string());
        }
        if cleaned.is_empty() {
            return Err(AttrError::Message(format!(
                "attribute '{key}' has no value"
            )));
        }
        match kind(key) {
            Kind::Package => push_unique(&mut self.package, cleaned),
            Kind::Manufacturer => push_unique(&mut self.manufacturer, cleaned),
            Kind::Attribute => {
                let slot = self.attributes.entry(key.to_string()).or_default();
                push_unique(slot, cleaned);
            }
        }
        Ok(())
    }
}

enum Kind {
    Package,
    Manufacturer,
    Attribute,
}

fn kind(key: &str) -> Kind {
    match folded(key).as_str() {
        "package" | "specification" | "componentspecification" => Kind::Package,
        "manufacturer" | "brand" | "componentbrand" => Kind::Manufacturer,
        _ => Kind::Attribute,
    }
}

fn folded(key: &str) -> String {
    key.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .flat_map(|c| c.to_lowercase())
        .collect()
}

fn push_unique(slot: &mut Vec<String>, values: Vec<String>) {
    for value in values {
        if !slot.iter().any(|existing| existing == &value) {
            slot.push(value);
        }
    }
}

fn string_values(value: &Value) -> Result<Vec<String>, AttrError> {
    match value {
        Value::String(text) => Ok(vec![text.clone()]),
        Value::Array(items) => {
            if items.is_empty() {
                return Err(AttrError::Message("value list is empty".into()));
            }
            items.iter().map(one_string).collect()
        }
        Value::Number(_) => Err(AttrError::Message(
            "a bare number is not a catalog value".into(),
        )),
        _ => Err(AttrError::Message(
            "expected a string or a list of strings".into(),
        )),
    }
}

fn one_string(value: &Value) -> Result<String, AttrError> {
    match value {
        Value::String(text) => Ok(text.clone()),
        Value::Number(_) => Err(AttrError::Message(
            "a bare number is not a catalog value".into(),
        )),
        _ => Err(AttrError::Message(
            "expected a string or a list of strings".into(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_object_splits_package_and_keeps_catalog_spelling() {
        let parsed = parse_product_attributes_text(
            r#"{"package":"0805","manufacturer":"Yageo","Resistance":"10kΩ","Tolerance":["±1%","±5%"]}"#,
        )
        .unwrap();
        assert_eq!(parsed.package, vec!["0805".to_string()]);
        assert_eq!(parsed.manufacturer, vec!["Yageo".to_string()]);
        assert_eq!(
            parsed.attributes.get("Resistance").unwrap(),
            &vec!["10kΩ".to_string()]
        );
        assert_eq!(parsed.attributes.get("Tolerance").unwrap().len(), 2);
    }

    #[test]
    fn a_repeated_param_adds_a_value_and_a_bare_number_is_rejected() {
        let mut parsed = parse_product_attributes_text(r#"{"Package":["0805"]}"#).unwrap();
        add_param(&mut parsed, "package=0603").unwrap();
        add_param(&mut parsed, "Resistance=10kΩ").unwrap();
        add_param(&mut parsed, "Resistance=9.1kΩ").unwrap();
        assert_eq!(parsed.package, vec!["0805".to_string(), "0603".to_string()]);
        assert_eq!(parsed.attributes.get("Resistance").unwrap().len(), 2);
        let bare = parse_product_attributes_text(r#"{"Resistance":10000}"#).unwrap_err();
        assert!(bare.to_string().contains("bare number"));
        let pair = add_param(&mut parsed, "Resistance").unwrap_err();
        assert!(pair.to_string().contains("key=value"));
    }

    #[test]
    fn the_document_must_be_an_object_and_a_list_must_hold_strings() {
        assert!(parse_product_attributes_text("[]")
            .unwrap_err()
            .to_string()
            .contains("JSON object"));
        assert!(parse_product_attributes_text(r#"{"Tolerance":[]}"#)
            .unwrap_err()
            .to_string()
            .contains("empty"));
        assert!(parse_product_attributes_text(r#"{"Tolerance":[1]}"#)
            .unwrap_err()
            .to_string()
            .contains("bare number"));
    }
}
