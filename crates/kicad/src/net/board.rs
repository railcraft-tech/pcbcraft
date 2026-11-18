use std::collections::HashMap;

use anyhow::Result;

use super::sexp::{atom_at, children, head, parse};

/// Net name to the distinct track widths on its segments and arcs.
pub fn track_widths(text: &str) -> Result<HashMap<String, Vec<f64>>> {
    let expr = parse(text)?;
    let mut names = HashMap::new();
    let mut widths: HashMap<String, Vec<f64>> = HashMap::new();
    for child in children(&expr).iter().skip(1) {
        match head(child) {
            Some("net") => {
                if let (Some(id), Some(name)) = (atom_at(child, 1), atom_at(child, 2)) {
                    names.insert(id.to_string(), name.to_string());
                }
            }
            Some("segment") | Some("arc") => {
                let Some(net_id) = field(child, "net") else {
                    continue;
                };
                let Some(width) = field(child, "width").and_then(|text| text.parse::<f64>().ok())
                else {
                    continue;
                };
                let name = names
                    .get(net_id)
                    .cloned()
                    .unwrap_or_else(|| net_id.to_string());
                widths.entry(name).or_default().push(width);
            }
            _ => {}
        }
    }
    // Segments may appear before the net table. Resolve ids that were stored raw.
    let mut resolved: HashMap<String, Vec<f64>> = HashMap::new();
    for (key, values) in widths {
        let name = names.get(&key).cloned().unwrap_or(key);
        resolved.entry(name).or_default().extend(values);
    }
    Ok(resolved)
}

fn field<'a>(expr: &'a super::sexp::SExpr, name: &str) -> Option<&'a str> {
    children(expr).iter().find_map(|child| {
        if head(child) == Some(name) {
            atom_at(child, 1)
        } else {
            None
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segment_and_arc_widths_follow_the_net_name() {
        let text = r#"
(kicad_pcb
  (net 1 "GND")
  (net 2 "VCC")
  (segment (start 0 0) (end 1 0) (width 0.25) (layer "F.Cu") (net 1))
  (segment (start 0 0) (end 0 1) (width 0.5) (layer "F.Cu") (net 1))
  (arc (start 0 0) (mid 1 1) (end 2 0) (width 0.3) (layer "F.Cu") (net 2))
)
"#;
        let widths = track_widths(text).unwrap();
        let mut gnd = widths["GND"].clone();
        gnd.sort_by(f64::total_cmp);
        assert_eq!(gnd, vec![0.25, 0.5]);
        assert_eq!(widths["VCC"], vec![0.3]);
    }
}
