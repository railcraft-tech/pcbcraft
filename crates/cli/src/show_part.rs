//! JSON view of one card from the local component database.

use pcbcraft_domain::parts::PartRecord;
use serde_json::{json, Value};

pub fn render(records: &[PartRecord]) -> anyhow::Result<String> {
    let value = match records {
        [one] => record_json(one),
        many => Value::Array(many.iter().map(record_json).collect()),
    };
    pretty(&value)
}

/// Search results are always an array, including when one part matched.
pub fn render_list(records: &[PartRecord]) -> anyhow::Result<String> {
    pretty(&Value::Array(records.iter().map(record_json).collect()))
}

fn pretty(value: &Value) -> anyhow::Result<String> {
    let mut text = serde_json::to_string_pretty(value)?;
    text.push('\n');
    Ok(text)
}

fn record_json(record: &PartRecord) -> Value {
    json!({
        "supplier": record.supplier.as_str(),
        "part": record.part_id,
        "mpn": record.mpn,
        "manufacturer": record.manufacturer,
        "package": record.package,
        "category": record.category,
        "description": record.description,
        "stock": record.stock,
        "price": record.price,
        "library": record.library,
        "parameters": record.parameters.iter().map(|param| json!({
            "name": param.name,
            "value": param.value,
            "si": param.si,
        })).collect::<Vec<_>>()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use pcbcraft_domain::parts::{ElecParam, Supplier};

    #[test]
    fn one_card_is_a_json_object_with_every_parameter() {
        let text = render(&[PartRecord {
            supplier: Supplier::Lcsc,
            part_id: "C123".into(),
            mpn: "RC0603".into(),
            manufacturer: "Yageo".into(),
            package: "0603".into(),
            category: "Resistors".into(),
            description: "10k".into(),
            stock: Some(20),
            price: Some(0.01),
            library: Some("basic".into()),
            parameters: vec![
                ElecParam {
                    name: "Resistance".into(),
                    value: "10 kΩ".into(),
                    si: Some(10_000.0),
                },
                ElecParam {
                    name: "Dielectric".into(),
                    value: "-".into(),
                    si: None,
                },
            ],
        }])
        .unwrap();
        let value: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value["supplier"], "lcsc");
        assert_eq!(value["part"], "C123");
        assert_eq!(value["parameters"][0]["name"], "Resistance");
        assert_eq!(value["parameters"][0]["si"], 10_000.0);
        assert!(value["parameters"][1]["si"].is_null());
    }
}
