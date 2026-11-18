use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use pcbcraft_application::ports::{UsedPart, UsedParts};
use pcbcraft_domain::parts::Supplier;

use crate::fabrication::KiCad;
use crate::net::{collect_sheets, components_and_sheets};

impl UsedParts for KiCad {
    fn list(&self, project: &Path) -> Result<Vec<UsedPart>> {
        let schematic = schematic_of(project);
        let sheets = collect_sheets(&schematic)?;
        let mut parts = Vec::new();
        for sheet in sheets {
            let text = std::fs::read_to_string(&sheet)
                .with_context(|| format!("cannot read {}", sheet.display()))?;
            let (components, _) = components_and_sheets(&text)?;
            for component in components {
                if let Some(id) = nonempty(component.lcsc) {
                    parts.push(UsedPart {
                        supplier: Supplier::Lcsc,
                        part_id: id,
                    });
                }
                if let Some(id) = nonempty(component.pcbway) {
                    parts.push(UsedPart {
                        supplier: Supplier::Pcbway,
                        part_id: id,
                    });
                }
            }
        }
        parts.sort_by(|left, right| {
            left.part_id
                .cmp(&right.part_id)
                .then(left.supplier.as_str().cmp(right.supplier.as_str()))
        });
        parts.dedup();
        Ok(parts)
    }
}

fn nonempty(value: Option<String>) -> Option<String> {
    value.filter(|text| !text.is_empty())
}

fn schematic_of(path: &Path) -> PathBuf {
    match path.extension().and_then(|ext| ext.to_str()) {
        Some("kicad_pro") | Some("kicad_pcb") => path.with_extension("kicad_sch"),
        _ => path.to_path_buf(),
    }
}
