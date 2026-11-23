use std::fs;
use std::path::Path;

use anyhow::{bail, Context, Result};
use pcbcraft_application::parts::{
    download_datasheet as save_datasheet, download_models, search_all, DownloadJob, PartKind,
    PartOutcome,
};
use pcbcraft_domain::parts::{
    add_param, parse_product_attributes, parse_product_attributes_text, parse_spec,
    ProductAttributes, Query,
};
use pcbcraft_kicad::EasyedaLibrary;
use pcbcraft_suppliers::{EasyedaClient, LcscCatalog, LcscDatasheets, PcbwayCatalog};

use super::args::{AltArgs, DatasheetArgs, DownloadArgs, SearchArgs};
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

pub fn download_datasheet(cwd: &Path, args: &DatasheetArgs) -> CommandOutput {
    CommandOutput::from_result(download_datasheet_result(cwd, args))
}

fn download_datasheet_result(cwd: &Path, args: &DatasheetArgs) -> Result<CommandOutput> {
    if !args.supplier.eq_ignore_ascii_case("lcsc") {
        bail!(
            "unknown supplier '{}'. parts download-datasheet supports lcsc",
            args.supplier
        );
    }
    let dir = place(cwd, &args.output_dir);
    let report = save_datasheet(
        &args.part,
        &dir,
        &LcscDatasheets,
        &LcscDatasheets,
        &crate::pdf2md::Pymupdf,
    )?;
    let mut stdout = String::new();
    for path in &report.files {
        stdout.push_str(&display_path(cwd, path));
        stdout.push('\n');
    }
    let stderr = match report.warning {
        Some(warning) if warning.ends_with('\n') => warning,
        Some(warning) => format!("{warning}\n"),
        None => String::new(),
    };
    Ok(CommandOutput::status(0, stdout, stderr))
}

pub fn download(cwd: &Path, args: &DownloadArgs, color: bool) -> CommandOutput {
    CommandOutput::from_result(download_result(cwd, args, color))
}

fn download_result(cwd: &Path, args: &DownloadArgs, color: bool) -> Result<CommandOutput> {
    if args.lcsc.is_empty() {
        bail!("pass at least one LCSC number");
    }
    let (anchor, table) = match &args.project {
        Some(path) => {
            let project = place(cwd, path);
            if !project.is_file() {
                bail!("project file does not exist: {}", project.display());
            }
            let anchor = project
                .parent()
                .context("project path has no directory")?
                .canonicalize()
                .context("could not open the project directory")?;
            (anchor.clone(), Some(anchor.join("fp-lib-table")))
        }
        None => (
            cwd.canonicalize()
                .context("could not open the working directory")?,
            None,
        ),
    };
    let library = match &args.library {
        Some(path) => place(cwd, path),
        None => cwd.join(".pcbcraft"),
    };
    fs::create_dir_all(&library)
        .with_context(|| format!("could not create {}", library.display()))?;
    let library = library
        .canonicalize()
        .with_context(|| format!("could not open {}", library.display()))?;
    let job = DownloadJob {
        ids: args.lcsc.clone(),
        pretty_dir: library.join("EasyEDA.pretty"),
        shapes_dir: library.join("EasyEDA.3dshapes"),
        anchor,
        table,
    };
    let report = download_models(&job, &EasyedaClient, &EasyedaLibrary);
    let failed = report
        .parts
        .iter()
        .any(|part| part.kind == PartKind::Failed)
        || report.register_error.is_some();
    Ok(CommandOutput::status(
        if failed { 1 } else { 0 },
        format_report(cwd, &report, color),
        "",
    ))
}

fn format_report(
    cwd: &Path,
    report: &pcbcraft_application::parts::DownloadReport,
    color: bool,
) -> String {
    let mut out = format!("library  {}\n\n", display_path(cwd, &report.pretty_dir));
    for part in &report.parts {
        let (word, code) = match part.kind {
            PartKind::Model => ("model", 32),
            PartKind::Footprint => ("footprint", 33),
            PartKind::Failed => ("failed", 31),
        };
        out.push_str(&format!("{}  {}\n", part.lcsc, paint(word, code, color)));
        if let Some(id) = &part.footprint {
            out.push_str(&dim_line(id, color));
        }
        if let Some(step) = &part.step {
            out.push_str(&dim_line(&display_path(cwd, step), color));
        }
        for note in &part.notes {
            out.push_str(&dim_line(note, color));
        }
        out.push('\n');
    }
    if let Some(err) = &report.register_error {
        out.push_str(&dim_line(err, color));
    } else if let Some(table) = &report.registered {
        out.push_str(&format!(
            "registered EasyEDA in {}\n",
            display_path(cwd, table)
        ));
    } else if report
        .parts
        .iter()
        .any(|part| part.kind != PartKind::Failed)
    {
        out.push_str(&format!(
            "(lib (name \"EasyEDA\")(type \"KiCad\")(uri \"{}\")(options \"\")(descr \"Footprints downloaded from EasyEDA by pcbcraft\"))\n",
            report.lib_uri
        ));
    }
    out.push_str(&summary(&report.parts));
    out.push('\n');
    out
}

fn summary(parts: &[PartOutcome]) -> String {
    if parts.len() == 1 {
        return match parts[0].kind {
            PartKind::Model => "footprint and 3D model saved".to_string(),
            PartKind::Footprint => "footprint saved, 3D model missing".to_string(),
            PartKind::Failed => "footprint was not saved".to_string(),
        };
    }
    let saved = parts
        .iter()
        .filter(|part| part.kind != PartKind::Failed)
        .count();
    let models = parts
        .iter()
        .filter(|part| part.kind == PartKind::Model)
        .count();
    let failed = parts
        .iter()
        .filter(|part| part.kind == PartKind::Failed)
        .count();
    let word = if saved == 1 {
        "footprint"
    } else {
        "footprints"
    };
    format!("{saved} {word}, {models} with a 3D model, {failed} failed")
}

fn display_path(cwd: &Path, path: &Path) -> String {
    let anchor = cwd.canonicalize().unwrap_or_else(|_| cwd.to_path_buf());
    path.strip_prefix(&anchor)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn paint(text: &str, code: u8, color: bool) -> String {
    if color {
        format!("\u{1b}[{code}m{text}\u{1b}[0m")
    } else {
        text.to_string()
    }
}

fn dim_line(text: &str, color: bool) -> String {
    format!("  {}\n", paint(text, 90, color))
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

    #[test]
    fn the_download_report_names_each_part_and_counts_the_result() {
        let cwd = std::env::temp_dir();
        let report = pcbcraft_application::parts::DownloadReport {
            pretty_dir: cwd.join(".pcbcraft/EasyEDA.pretty"),
            lib_uri: "${KIPRJMOD}/.pcbcraft/EasyEDA.pretty".into(),
            registered: None,
            register_error: None,
            parts: vec![
                PartOutcome {
                    lcsc: "C2040".into(),
                    kind: PartKind::Model,
                    footprint: Some("EasyEDA:R0805".into()),
                    step: Some(cwd.join(".pcbcraft/EasyEDA.3dshapes/R0805.step")),
                    notes: vec![],
                },
                PartOutcome {
                    lcsc: "C15127".into(),
                    kind: PartKind::Footprint,
                    footprint: Some("EasyEDA:SOT-23".into()),
                    step: None,
                    notes: vec!["no 3D model in the EasyEDA footprint".into()],
                },
                PartOutcome {
                    lcsc: "C00000".into(),
                    kind: PartKind::Failed,
                    footprint: None,
                    step: None,
                    notes: vec!["no component for this LCSC number".into()],
                },
            ],
        };
        let text = format_report(&cwd, &report, false);
        assert!(text.contains("C2040  model\n  EasyEDA:R0805\n"));
        assert!(text.contains("C15127  footprint\n"));
        assert!(text.contains("C00000  failed\n"));
        assert!(text.contains("2 footprints, 1 with a 3D model, 1 failed\n"));
        assert!(text.contains("(lib (name \"EasyEDA\")"));
        assert!(!text.contains('\u{1b}'));
        let colored = format_report(&cwd, &report, true);
        assert!(colored.contains("\u{1b}[32mmodel\u{1b}[0m"));
    }

    #[test]
    fn a_missing_project_file_is_a_command_error() {
        let output = download(
            &std::env::temp_dir(),
            &DownloadArgs {
                lcsc: vec!["C2040".into()],
                library: None,
                project: Some(std::path::PathBuf::from("missing-pcbcraft.kicad_pro")),
            },
            false,
        );
        assert_eq!(output.code, 1);
        assert!(output.stderr.contains("does not exist"));
        assert!(output.stdout.is_empty());
    }

    #[test]
    fn download_datasheet_rejects_an_unknown_supplier_without_writing() {
        let dir = std::env::temp_dir().join(format!(
            "pcbcraft-datasheet-supplier-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let output = download_datasheet(
            &dir,
            &DatasheetArgs {
                supplier: "jlcpcb".into(),
                part: "C55266".into(),
                output_dir: dir.join("out"),
            },
        );
        assert_eq!(output.code, 1);
        assert!(output.stderr.contains("lcsc"), "{}", output.stderr);
        assert!(!dir.join("out").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
