use anyhow::Result;
use pcbcraft_domain::validate::{Severity, Violation, ViolationItem};
use serde_json::Value;

pub fn parse_report(text: &str, check: &str) -> Result<Vec<Violation>> {
    let value: Value = serde_json::from_str(text)?;
    let Some(items) = value.get("violations").and_then(Value::as_array) else {
        return Ok(Vec::new());
    };
    let mut violations = Vec::new();
    for item in items {
        let excluded = item
            .get("excluded")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let severity = if excluded {
            Severity::Exclusion
        } else {
            Severity::parse(
                item.get("severity")
                    .and_then(Value::as_str)
                    .unwrap_or("error"),
            )
        };
        let mut entries = Vec::new();
        if let Some(nodes) = item.get("items").and_then(Value::as_array) {
            for node in nodes {
                let pos = node.get("pos");
                entries.push(ViolationItem {
                    description: node
                        .get("description")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string(),
                    uuid: node.get("uuid").and_then(Value::as_str).map(str::to_string),
                    x_mm: pos.and_then(|pos| pos.get("x")).and_then(Value::as_f64),
                    y_mm: pos.and_then(|pos| pos.get("y")).and_then(Value::as_f64),
                });
            }
        }
        violations.push(Violation {
            check: check.to_string(),
            severity,
            kind: item
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            description: item
                .get("description")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            items: entries,
        });
    }
    Ok(violations)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_an_erc_violation() {
        let text = r#"{"violations":[{"type":"pin_not_connected","description":"Pin not connected","severity":"error","items":[{"description":"R1 pin 1","uuid":"abc","pos":{"x":1.5,"y":2.5}}]}]}"#;
        let violations = parse_report(text, "erc").unwrap();
        assert_eq!(violations[0].kind, "pin_not_connected");
        assert_eq!(violations[0].items[0].x_mm, Some(1.5));
    }
}
