use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use pcbcraft_application::ports::{PartStock, TestPrinter};
use pcbcraft_application::validate::{
    known_types, order, relative_path, run_tests_skipping, write_one, write_scaffold, LoadedTest,
    TestContext,
};
use pcbcraft_domain::parts::SupplierCatalog;
use pcbcraft_domain::validate::TestFile;
use pcbcraft_kicad::{KiCad, ProjectCards};
use pcbcraft_suppliers::{LcscCatalog, PcbwayCatalog};

use super::args::{InitTypeArgs, RunArgs, TestsDir};
use super::output::{place, CommandOutput};
use crate::printer::{memory_label, ConsolePrinter};

pub fn run(cwd: &Path, args: &RunArgs, color: bool) -> CommandOutput {
    CommandOutput::from_result(run_result(cwd, args, color))
}

pub fn list(cwd: &Path, args: &TestsDir) -> CommandOutput {
    CommandOutput::from_result(list_result(cwd, args))
}

pub fn init_defaults(cwd: &Path, args: &TestsDir) -> CommandOutput {
    CommandOutput::from_result(init_defaults_result(cwd, args))
}

pub fn init_type(cwd: &Path, args: &InitTypeArgs) -> CommandOutput {
    CommandOutput::from_result(init_type_result(cwd, args))
}

fn run_result(cwd: &Path, args: &RunArgs, color: bool) -> Result<CommandOutput> {
    let directory = test_dir(cwd, args.directory.as_deref());
    if !directory.is_dir() {
        return Ok(CommandOutput::error(
            2,
            format!("no directory {}\n", directory.display()),
        ));
    }
    let mut files = Vec::new();
    collect_json(&directory, &directory, &mut files)?;
    if let Some(name) = &args.name {
        let wanted = keep_named(&mut files, name);
        if files.is_empty() {
            return Ok(CommandOutput::error(2, format!("no test '{wanted}'\n")));
        }
    } else if files.is_empty() {
        return Ok(CommandOutput::error(
            2,
            format!("no JSON files in {}\n", directory.display()),
        ));
    }
    let mut tests = Vec::new();
    let mut parse_errors = Vec::new();
    for (name, path) in files {
        let text = fs::read_to_string(&path)?;
        match serde_json::from_str::<TestFile>(&text) {
            Ok(file) => {
                let mut spec = file.spec;
                absolutize(&path, &mut spec);
                tests.push(LoadedTest {
                    name,
                    type_name: file.type_name,
                    spec,
                });
            }
            Err(err) => parse_errors.push(format!("{name}: {err}")),
        }
    }
    let mut stdout = Vec::new();
    let mut printer = ConsolePrinter::new(&mut stdout, color);
    if !parse_errors.is_empty() {
        printer.schema_errors(&parse_errors);
        drop(printer);
        return Ok(CommandOutput::status(2, utf8(stdout), ""));
    }
    let kicad = KiCad::from_env();
    let stock = LiveStock {
        lcsc: LcscCatalog::default(),
        pcbway: PcbwayCatalog,
    };
    let cards = ProjectCards;
    let ctx = TestContext {
        fabrication: &kicad,
        used_parts: &kicad,
        stock: &stock,
        graph: &kicad,
        cards: &cards,
        boards: &kicad,
    };
    let skipped = match load_skips(cwd) {
        Ok(skipped) => skipped,
        Err(err) => {
            return Ok(CommandOutput::error(2, format!("pcbcraft: {err:#}\n")));
        }
    };
    let totals = run_tests_skipping(tests, &ctx, &mut printer, &skipped)?;
    if totals.blocked {
        drop(printer);
        return Ok(CommandOutput::status(2, utf8(stdout), ""));
    }
    printer.summary(
        totals.elapsed,
        &memory_label(),
        totals.failed == 0,
        totals.tests,
        totals.assertions,
        totals.skipped,
        totals.warnings,
    );
    drop(printer);
    let mut stderr = String::new();
    for entry in totals.unmatched_skips {
        stderr.push_str(&format!("skipped entry '{entry}' did not match a test\n"));
    }
    let code = if totals.failed == 0 { 0 } else { 1 };
    Ok(CommandOutput::status(code, utf8(stdout), stderr))
}

fn list_result(cwd: &Path, args: &TestsDir) -> Result<CommandOutput> {
    let directory = test_dir(cwd, args.directory.as_deref());
    let mut loaded = Vec::new();
    let mut broken = Vec::new();
    if directory.is_dir() {
        let mut files = Vec::new();
        collect_json(&directory, &directory, &mut files)?;
        for (name, path) in files {
            let text = fs::read_to_string(&path)?;
            match serde_json::from_str::<TestFile>(&text) {
                Ok(file) => loaded.push(LoadedTest {
                    name,
                    type_name: file.type_name,
                    spec: file.spec,
                }),
                Err(err) => broken.push((name, err.to_string())),
            }
        }
    }
    broken.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(CommandOutput::text(format_test_list(
        &order(loaded),
        &broken,
        known_types(),
    )))
}

fn init_defaults_result(cwd: &Path, args: &TestsDir) -> Result<CommandOutput> {
    let directory = test_dir(cwd, args.directory.as_deref());
    if directory.is_file() {
        bail!("{} is a file, not a directory", directory.display());
    }
    let project = discover_project(cwd)?;
    let project_path = project
        .clone()
        .unwrap_or_else(|| cwd.join("project.kicad_pro"));
    let relative = relative_path(&directory, &project_path);
    let created = write_scaffold(&directory, &relative)?;
    let mut stderr = String::new();
    if created.is_empty() {
        stderr.push_str(&format!(
            "{} already has the default tests\n",
            directory.display()
        ));
    } else {
        for path in &created {
            stderr.push_str(&format!("Created {}\n", path.display()));
        }
        if project.is_none() {
            stderr.push_str("Set spec.project in those files to your .kicad_pro.\n");
        }
    }
    Ok(CommandOutput::note(stderr))
}

fn init_type_result(cwd: &Path, args: &InitTypeArgs) -> Result<CommandOutput> {
    let (Some(type_name), Some(name)) = (args.type_name.as_deref(), args.name.as_deref()) else {
        return Ok(CommandOutput::error(2, missing_type_choice()));
    };
    let directory = test_dir(cwd, args.directory.as_deref());
    if directory.is_file() {
        bail!("{} is a file, not a directory", directory.display());
    }
    if !known_types().contains(&type_name) {
        bail!("unknown test type '{type_name}'");
    }
    let project = discover_project(cwd)?;
    let project_path = project
        .clone()
        .unwrap_or_else(|| cwd.join("project.kicad_pro"));
    let relative = relative_path(&directory, &project_path);
    let mut stderr = String::new();
    match write_one(&directory, type_name, name, &relative)? {
        Some(path) => {
            stderr.push_str(&format!("Created {}\n", path.display()));
            if project.is_none() {
                stderr.push_str("Set spec.project in those files to your .kicad_pro.\n");
            }
        }
        None => {
            let file_name = if name.ends_with(".json") {
                name.to_string()
            } else {
                format!("{name}.json")
            };
            stderr.push_str(&format!(
                "{} already exists\n",
                directory.join(file_name).display()
            ));
        }
    }
    Ok(CommandOutput::note(stderr))
}

pub fn missing_type_choice() -> String {
    format!(
        "Choose a test type and a test name (file name without an extension).\n\n{}",
        super::args::type_list_help()
    )
}

fn test_dir(cwd: &Path, directory: Option<&Path>) -> PathBuf {
    match directory {
        Some(path) => place(cwd, path),
        None => cwd.join("tests/pcbcraft"),
    }
}

fn load_skips(cwd: &Path) -> Result<Vec<String>> {
    let path = cwd.join("pcbcraft.yaml");
    if !path.is_file() {
        return Ok(Vec::new());
    }
    let text =
        fs::read_to_string(&path).with_context(|| format!("cannot read {}", path.display()))?;
    let config =
        pcbcraft_domain::config::parse_config(&text).map_err(|err| anyhow::anyhow!(err))?;
    Ok(config.skipped)
}

fn discover_project(cwd: &Path) -> Result<Option<PathBuf>> {
    let mut found = Vec::new();
    for entry in fs::read_dir(cwd).with_context(|| format!("{}", cwd.display()))? {
        let entry = entry?;
        if entry.path().extension().and_then(|ext| ext.to_str()) == Some("kicad_pro") {
            found.push(entry.path());
        }
    }
    match found.len() {
        0 => Ok(None),
        1 => Ok(Some(found.remove(0))),
        _ => Ok(None),
    }
}

fn absolutize(json_path: &Path, spec: &mut serde_json::Value) {
    let Some(project) = spec.get("project").and_then(|value| value.as_str()) else {
        return;
    };
    let path = Path::new(project);
    if path.is_absolute() {
        return;
    }
    if let Some(parent) = json_path.parent() {
        let absolute = parent.join(path);
        spec["project"] = serde_json::Value::String(absolute.display().to_string());
    }
}

pub fn keep_named(files: &mut Vec<(String, PathBuf)>, name: &str) -> String {
    let wanted = name.strip_suffix(".json").unwrap_or(name).to_string();
    files.retain(|(test_name, _)| test_name == &wanted);
    wanted
}

fn collect_json(root: &Path, dir: &Path, out: &mut Vec<(String, PathBuf)>) -> Result<()> {
    let mut entries = fs::read_dir(dir)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            collect_json(root, &path, out)?;
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("json") {
            let name = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .with_extension("")
                .to_string_lossy()
                .replace('\\', "/");
            out.push((name, path));
        }
    }
    Ok(())
}

fn utf8(bytes: Vec<u8>) -> String {
    String::from_utf8_lossy(&bytes).into_owned()
}

struct LiveStock {
    lcsc: LcscCatalog,
    pcbway: PcbwayCatalog,
}

impl PartStock for LiveStock {
    fn lookup(
        &self,
        supplier: pcbcraft_domain::parts::Supplier,
        part_id: &str,
    ) -> Result<Option<u64>> {
        let records = match supplier {
            pcbcraft_domain::parts::Supplier::Lcsc => self.lcsc.fetch(&[part_id.to_string()])?,
            pcbcraft_domain::parts::Supplier::Pcbway => {
                self.pcbway.fetch(&[part_id.to_string()])?
            }
        };
        Ok(records.into_iter().next().and_then(|record| record.stock))
    }
}

pub fn format_test_list(
    tests: &[LoadedTest],
    broken: &[(String, String)],
    types: &[&str],
) -> String {
    let mut out = String::new();
    if tests.is_empty() && broken.is_empty() {
        out.push_str("(no tests)\n");
    }
    for test in tests {
        out.push_str(&format!("{}  {}\n", test.name, test.type_name));
    }
    for (name, err) in broken {
        out.push_str(&format!("{name}  invalid JSON: {err}\n"));
    }
    out.push('\n');
    out.push_str("Types\n");
    for type_name in types {
        out.push_str(&format!("{type_name}\n"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{format_test_list, keep_named};
    use pcbcraft_application::validate::{known_types, order, LoadedTest};
    use std::path::PathBuf;

    #[test]
    fn the_list_puts_project_tests_above_the_known_types() {
        let tests = order(vec![
            LoadedTest {
                name: "drc-test".into(),
                type_name: "DRCValidator".into(),
                spec: serde_json::json!({}),
            },
            LoadedTest {
                name: "erc-test".into(),
                type_name: "ERCValidator".into(),
                spec: serde_json::json!({}),
            },
        ]);
        let text = format_test_list(&tests, &[], &["ERCValidator", "DRCValidator"]);
        assert_eq!(
            text,
            "\
erc-test  ERCValidator
drc-test  DRCValidator

Types
ERCValidator
DRCValidator
"
        );
    }

    #[test]
    fn a_missing_type_asks_for_the_type_and_the_file_name() {
        let text = super::missing_type_choice();
        assert!(text.contains("Choose a test type and a test name"));
        assert!(text.contains("without an extension"));
        for name in known_types() {
            assert!(text.contains(name), "{name}");
        }
    }

    #[test]
    fn name_selects_one_file_and_not_a_prefix() {
        let mut files = vec![
            ("erc-test".into(), PathBuf::from("erc-test.json")),
            (
                "erc-test-extra".into(),
                PathBuf::from("erc-test-extra.json"),
            ),
            (
                "checks/erc-test".into(),
                PathBuf::from("checks/erc-test.json"),
            ),
        ];
        let wanted = keep_named(&mut files, "erc-test.json");
        assert_eq!(wanted, "erc-test");
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].0, "erc-test");
    }
}
