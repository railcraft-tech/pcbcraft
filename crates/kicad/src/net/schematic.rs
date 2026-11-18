use anyhow::Result;
use pcbcraft_domain::net::Component;

use super::sexp::{atom_at, children, head, parse, SExpr};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SheetRef {
    pub file: String,
}

pub fn components_and_sheets(text: &str) -> Result<(Vec<Component>, Vec<SheetRef>)> {
    let expr = parse(text)?;
    let mut components = Vec::new();
    let mut sheets = Vec::new();
    walk(&expr, false, &mut components, &mut sheets);
    Ok((components, sheets))
}

fn walk(
    expr: &SExpr,
    in_library: bool,
    components: &mut Vec<Component>,
    sheets: &mut Vec<SheetRef>,
) {
    let Some(name) = head(expr) else {
        return;
    };
    if name == "lib_symbols" {
        return;
    }
    if !in_library && name == "symbol" {
        if let Some(component) = component_from(expr) {
            components.push(component);
        }
    }
    if name == "property" {
        if let (Some(key), Some(value)) = (atom_at(expr, 1), atom_at(expr, 2)) {
            if key == "Sheetfile" || key == "Sheet file" {
                sheets.push(SheetRef {
                    file: value.to_string(),
                });
            }
        }
    }
    for child in children(expr).iter().skip(1) {
        walk(child, in_library, components, sheets);
    }
}

fn component_from(expr: &SExpr) -> Option<Component> {
    let mut symbol = String::new();
    let mut props = Vec::new();
    let mut dnp = false;
    let mut in_bom = true;
    let mut in_pos_files = true;
    for child in children(expr).iter().skip(1) {
        match head(child) {
            Some("lib_id") => {
                if let Some(id) = atom_at(child, 1) {
                    symbol = id.to_string();
                }
            }
            Some("dnp") => {
                dnp = atom_at(child, 1) == Some("yes");
            }
            Some("in_bom") => {
                in_bom = atom_at(child, 1) != Some("no");
            }
            Some("in_pos_files") => {
                in_pos_files = atom_at(child, 1) != Some("no");
            }
            Some("property") => {
                if let (Some(key), Some(value)) = (atom_at(child, 1), atom_at(child, 2)) {
                    props.push((key.to_string(), value.to_string()));
                }
            }
            _ => {}
        }
    }
    let reference = prop(&props, &["Reference"])?;
    if reference.starts_with('#') {
        return None;
    }
    Some(Component {
        reference,
        symbol,
        value: prop(&props, &["Value"]).unwrap_or_default(),
        footprint: prop(&props, &["Footprint"]).unwrap_or_default(),
        description: prop(&props, &["Description", "Datasheet"]).unwrap_or_default(),
        lcsc: prop(&props, &["LCSC", "LCSC Part #", "LCSC Part Number"]),
        pcbway: prop(&props, &["PCBWay", "PCBWay Part #", "PCBWay Part Number"]),
        dnp,
        in_bom,
        in_pos_files,
    })
}

fn prop(props: &[(String, String)], keys: &[&str]) -> Option<String> {
    for key in keys {
        if let Some((_, value)) = props
            .iter()
            .find(|(name, value)| name == key && !value.is_empty())
        {
            return Some(value.clone());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_lcsc_and_skips_the_library_symbol() {
        let text = r#"
(kicad_sch
  (lib_symbols
    (symbol "Device:R"
      (property "Reference" "R" (at 0 0 0))
      (property "Value" "R" (at 0 0 0))
    )
  )
  (symbol
    (lib_id "Device:R")
    (property "Reference" "R1" (at 0 0 0))
    (property "Value" "10k" (at 0 0 0))
    (property "Footprint" "Resistor_SMD:R_0805" (at 0 0 0))
    (property "LCSC Part #" "C17414" (at 0 0 0))
    (property "Description" "resistor" (at 0 0 0))
  )
  (sheet
    (property "Sheetfile" "child.kicad_sch" (at 0 0 0))
  )
)
"#;
        let (components, sheets) = components_and_sheets(text).unwrap();
        assert_eq!(components.len(), 1);
        assert_eq!(components[0].reference, "R1");
        assert_eq!(components[0].lcsc.as_deref(), Some("C17414"));
        assert_eq!(components[0].symbol, "Device:R");
        assert!(!components[0].dnp);
        assert_eq!(sheets[0].file, "child.kicad_sch");
    }

    #[test]
    fn a_dnp_symbol_is_marked() {
        let text = r#"
(kicad_sch
  (symbol
    (lib_id "Device:C")
    (dnp yes)
    (property "Reference" "C16" (at 0 0 0))
    (property "Value" "DNP" (at 0 0 0))
  )
)
"#;
        let (components, _) = components_and_sheets(text).unwrap();
        assert!(components[0].dnp);
    }

    #[test]
    fn a_mounting_hole_keeps_the_bom_and_position_flags() {
        let text = r#"
(kicad_sch
  (symbol
    (lib_id "Mechanical:MountingHole")
    (in_bom no)
    (in_pos_files no)
    (property "Reference" "H1" (at 0 0 0))
    (property "Value" "MountingHole_3.2mm_M3" (at 0 0 0))
    (property "Footprint" "MountingHole:MountingHole_3.2mm_M3" (at 0 0 0))
  )
)
"#;
        let (components, _) = components_and_sheets(text).unwrap();
        assert!(!components[0].in_bom);
        assert!(!components[0].in_pos_files);
        assert_eq!(
            components[0].footprint,
            "MountingHole:MountingHole_3.2mm_M3"
        );
    }

    #[test]
    fn missing_flags_mean_the_part_is_in_the_bom_and_the_position_file() {
        let text = r#"
(kicad_sch
  (symbol
    (lib_id "Device:R")
    (property "Reference" "R1" (at 0 0 0))
  )
)
"#;
        let (components, _) = components_and_sheets(text).unwrap();
        assert!(components[0].in_bom);
        assert!(components[0].in_pos_files);
    }
}
