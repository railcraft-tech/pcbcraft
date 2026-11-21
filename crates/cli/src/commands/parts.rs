use std::fs;
use std::path::Path;

use anyhow::Result;
use pcbcraft_application::parts::search_all;
use pcbcraft_domain::parts::{parse_spec, Query};
use pcbcraft_suppliers::{LcscCatalog, PcbwayCatalog};

use super::args::AltArgs;
use super::output::{place, CommandOutput};

pub fn alternatives(cwd: &Path, args: &AltArgs) -> CommandOutput {
    CommandOutput::from_result(alternatives_result(cwd, args))
}

fn alternatives_result(cwd: &Path, args: &AltArgs) -> Result<CommandOutput> {
    let text = fs::read_to_string(place(cwd, &args.spec))?;
    let spec = parse_spec(&text)?;
    let lcsc = LcscCatalog {
        in_stock: !args.include_out_of_stock,
        basic_only: args.basic,
    };
    let found = search_all(&spec.items, &lcsc, &PcbwayCatalog, args.limit)?;
    let stdout = if args.format.as_deref() == Some("yaml") {
        to_yaml(&found)?
    } else {
        let mut out = String::new();
        for item in &found {
            out.push_str(&item.name);
            out.push('\n');
            if item.hits.is_empty() {
                out.push_str("  (none)\n");
                continue;
            }
            for hit in &item.hits {
                out.push_str(&format!(
                    "  {} {} {} {} stock={}\n",
                    hit.supplier,
                    hit.part_id,
                    hit.mpn,
                    hit.package,
                    hit.stock
                        .map(|n| n.to_string())
                        .unwrap_or_else(|| "-".into())
                ));
            }
        }
        out
    };
    let _ = Query::Lcsc;
    Ok(CommandOutput::text(stdout))
}

fn to_yaml(found: &[pcbcraft_application::parts::Found]) -> Result<String> {
    let mut out = String::from("items:\n");
    for item in found {
        out.push_str(&format!("  - name: {}\n    matches:\n", item.name));
        for hit in &item.hits {
            out.push_str(&format!(
                "      - supplier: {}\n        id: {}\n        mpn: \"{}\"\n        package: \"{}\"\n",
                hit.supplier, hit.part_id, hit.mpn, hit.package
            ));
        }
    }
    Ok(out)
}
