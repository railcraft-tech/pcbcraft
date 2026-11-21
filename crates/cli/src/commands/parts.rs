use std::fs;
use std::path::Path;

use anyhow::{bail, Context, Result};
use pcbcraft_application::parts::search_all;
use pcbcraft_domain::parts::{
    add_param, parse_product_attributes, parse_product_attributes_text, parse_spec,
    ProductAttributes, Query,
};
use pcbcraft_suppliers::{LcscCatalog, PcbwayCatalog};

use super::args::{AltArgs, SearchArgs};
use super::output::{place, CommandOutput};
use crate::show_part;

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

/// A non-empty pipe is the attribute document. An empty pipe leaves a file in place.
pub fn read_piped_attributes(args: &mut SearchArgs, piped: Option<String>) -> Result<()> {
    let Some(text) = piped else {
        return Ok(());
    };
    if text.trim().is_empty() {
        return Ok(());
    }
    if args.params.is_some() {
        bail!("pass product attributes as a file or on stdin, not both");
    }
    args.stdin_json = Some(text);
    Ok(())
}

pub fn search(cwd: &Path, args: &SearchArgs) -> CommandOutput {
    CommandOutput::from_result(search_result(cwd, args))
}

fn search_result(cwd: &Path, args: &SearchArgs) -> Result<CommandOutput> {
    if !args.supplier.eq_ignore_ascii_case("jlcpcb") {
        bail!(
            "unknown supplier '{}'. parts search supports jlcpcb",
            args.supplier
        );
    }
    if args.limit == 0 || args.limit > 50 {
        bail!("limit must be from 1 to 50");
    }
    let attributes = load_attributes(cwd, args)?;
    if args.query.trim().is_empty() && attributes_empty(&attributes) {
        bail!("pass a query or at least one attribute");
    }
    let catalog = LcscCatalog {
        in_stock: !args.include_out_of_stock,
        basic_only: args.basic,
    };
    let found = catalog.search_attributes(&args.query, &attributes, args.limit as u32)?;
    Ok(CommandOutput::text(show_part::render_list(&found)?))
}

fn attributes_empty(attributes: &ProductAttributes) -> bool {
    attributes.package.is_empty()
        && attributes.manufacturer.is_empty()
        && attributes.attributes.is_empty()
}

fn load_attributes(cwd: &Path, args: &SearchArgs) -> Result<ProductAttributes> {
    let sources = [
        args.params.is_some(),
        args.stdin_json.is_some(),
        args.attributes.is_some(),
    ]
    .into_iter()
    .filter(|present| *present)
    .count();
    if sources > 1 {
        bail!(
            "pass product attributes as a file, on stdin, or as an attributes object, not more than one"
        );
    }
    let mut parsed = if let Some(path) = &args.params {
        let text = fs::read_to_string(place(cwd, path))
            .with_context(|| format!("attributes {}", path.display()))?;
        parse_product_attributes_text(&text)?
    } else if let Some(text) = &args.stdin_json {
        parse_product_attributes_text(text)?
    } else if let Some(value) = &args.attributes {
        parse_product_attributes(value)?
    } else {
        ProductAttributes::default()
    };
    for pair in &args.param {
        add_param(&mut parsed, pair)?;
    }
    Ok(parsed)
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn args() -> SearchArgs {
        SearchArgs {
            supplier: "jlcpcb".into(),
            query: "resistor".into(),
            params: None,
            param: Vec::new(),
            attributes: None,
            stdin_json: None,
            limit: 10,
            include_out_of_stock: false,
            basic: false,
        }
    }

    #[test]
    fn json_and_param_flags_merge_and_two_documents_are_rejected() {
        let dir = std::env::temp_dir().join(format!(
            "pcbcraft-search-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("params.json");
        std::fs::write(&path, r#"{"package":"0805","Resistance":"10kΩ"}"#).unwrap();

        let mut from_file = args();
        from_file.params = Some(path);
        from_file.param = vec!["Tolerance=±1%".into(), "package=0603".into()];
        let parsed = load_attributes(&dir, &from_file).unwrap();
        assert_eq!(parsed.package, vec!["0805".to_string(), "0603".to_string()]);
        assert_eq!(
            parsed.attributes.get("Resistance").unwrap(),
            &vec!["10kΩ".to_string()]
        );
        assert_eq!(
            parsed.attributes.get("Tolerance").unwrap(),
            &vec!["±1%".to_string()]
        );

        let mut both = args();
        both.params = Some("params.json".into());
        both.stdin_json = Some("{}".into());
        assert!(load_attributes(&dir, &both)
            .unwrap_err()
            .to_string()
            .contains("not more than one"));

        let mut piped = args();
        piped.params = Some("params.json".into());
        assert!(read_piped_attributes(&mut piped, Some("{}".into()))
            .unwrap_err()
            .to_string()
            .contains("not both"));
        let mut empty_pipe = args();
        empty_pipe.params = Some("params.json".into());
        read_piped_attributes(&mut empty_pipe, Some(" \n".into())).unwrap();
        assert!(empty_pipe.stdin_json.is_none());

        let mut inline = args();
        inline.attributes = Some(json!({"manufacturer": "Yageo"}));
        inline.param = vec!["Resistance=10kΩ".into()];
        let parsed = load_attributes(&dir, &inline).unwrap();
        assert_eq!(parsed.manufacturer, vec!["Yageo".to_string()]);
        assert!(parsed.attributes.contains_key("Resistance"));

        let mut unknown = args();
        unknown.supplier = "pcbway".into();
        let output = search(&dir, &unknown);
        assert_eq!(output.code, 1);
        assert!(output.stderr.contains("jlcpcb"), "{}", output.stderr);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
