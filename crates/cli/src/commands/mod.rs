//! One function per CLI command. The MCP server calls the same functions.

use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use serde_json::{json, Value};

use crate::geometry_cmd::{DumpArgs, FindSymbolArgs, GeometryKind, OccupiedArgs};

use self::output::CommandOutput;

pub mod args;
pub mod board;
pub mod components;
pub mod export;
pub mod mcp;
pub mod net;
pub mod output;
pub mod parts;
pub mod tests_cmd;

pub struct Tool {
    pub name: &'static str,
    pub description: &'static str,
    pub input_schema: Value,
    pub call: fn(&Value) -> CommandOutput,
}

pub fn tools() -> &'static [Tool] {
    use std::sync::OnceLock;
    static TOOLS: OnceLock<Vec<Tool>> = OnceLock::new();
    TOOLS.get_or_init(registry).as_slice()
}

fn registry() -> Vec<Tool> {
    let files = json!({
        "input": {"type": "string", "description": "A .kicad_pro, .kicad_sch, or .kicad_pcb file."},
        "output": {"type": "string", "description": "Output file. Use - for stdout where the command allows it."},
        "schematic": {"type": "string"},
        "board": {"type": "string"}
    });
    let directory = json!({
        "directory": {"type": "string", "description": "Directory of JSON files. Defaults to tests/pcbcraft."}
    });
    vec![
        Tool {
            name: "board_setup",
            description: "Raise KiCad board rules to a JLCPCB, PCBWay, or custom JSON profile. Checks the files with kicad-cli and restores them if they do not load.",
            input_schema: schema(
                json!({
                    "profile": {"type": "string", "enum": ["jlcpcb", "pcbway", "custom"], "description": "jlcpcb, pcbway, or custom."},
                    "file": {"type": "string", "description": "JSON profile. Required when profile is custom."},
                    "input": {"type": "string", "description": "A .kicad_pro, .kicad_sch, or .kicad_pcb file."},
                    "mask": {"type": "string", "description": "Solder-mask color. Defaults to green."}
                }),
                &["profile"],
            ),
            call: board_setup,
        },
        Tool {
            name: "export_bom",
            description: "Write a JLCPCB-shaped bill of materials.",
            input_schema: schema(files.clone(), &[]),
            call: export_bom,
        },
        Tool {
            name: "export_pos",
            description: "Write the raw KiCad position file.",
            input_schema: schema(files.clone(), &[]),
            call: export_pos,
        },
        Tool {
            name: "export_cpl",
            description: "Correct rotation and offset, then write the five JLCPCB columns.",
            input_schema: schema(
                json!({
                    "input": {"type": "string", "description": "A project, a board, or an existing position CSV."},
                    "output": {"type": "string"},
                    "bom": {"type": "string"},
                    "local": {"type": "string"},
                    "db": {"type": "string"},
                    "corrections": {"type": "array", "items": {"type": "string"}},
                    "update": {"type": "boolean"},
                    "cache_dir": {"type": "string"},
                    "dry_run": {"type": "boolean"},
                    "quiet": {"type": "boolean"},
                    "schematic": {"type": "string"},
                    "board": {"type": "string"}
                }),
                &[],
            ),
            call: export_cpl,
        },
        Tool {
            name: "export_gerbers",
            description: "Write Gerbers and drill files into a zip.",
            input_schema: schema(files.clone(), &[]),
            call: export_gerbers,
        },
        Tool {
            name: "export_drawing",
            description: "Write a 2D drawing on one A4 page.",
            input_schema: schema(
                json!({
                    "input": {"type": "string"},
                    "output": {"type": "string"},
                    "side": {"type": "string", "description": "side or back. Omit it to put both faces on one page."},
                    "full": {"type": "boolean", "description": "Full color for the selected face."},
                    "board": {"type": "string"}
                }),
                &[],
            ),
            call: export_drawing,
        },
        Tool {
            name: "export_2d",
            description: "Write a square PNG of the orthographic 3D view.",
            input_schema: schema(
                json!({
                    "input": {"type": "string"},
                    "board": {"type": "string"},
                    "symbol": {"type": "string", "description": "Footprint reference to frame, for example J1."},
                    "side": {"type": "string", "description": "front or back. Defaults to front."},
                    "around_space": {"type": "string", "description": "Extra space around the symbol. Defaults to 5mm. Requires symbol."},
                    "width": {"type": "integer", "description": "Square image side in pixels. Defaults to 1024."},
                    "output": {"type": "string"}
                }),
                &[],
            ),
            call: export_2d,
        },
        Tool {
            name: "tests_run",
            description:
                "Run the JSON tests. Exit code 1 is a failure and 2 is a schema or setup error.",
            input_schema: schema(
                json!({
                    "directory": {"type": "string", "description": "Directory of JSON files. Defaults to tests/pcbcraft."},
                    "name": {"type": "string", "description": "One test file, without an extension. This is not a subtest."}
                }),
                &[],
            ),
            call: tests_run,
        },
        Tool {
            name: "tests_list",
            description: "List the tests in the directory, then the types this program can run.",
            input_schema: schema(directory.clone(), &[]),
            call: tests_list,
        },
        Tool {
            name: "tests_init_defaults",
            description: "Write erc-test, drc-test, and availability-test when they are missing.",
            input_schema: schema(directory.clone(), &[]),
            call: tests_init_defaults,
        },
        Tool {
            name: "tests_init_type",
            description:
                "Write one test file. Requires a type and a file name without an extension.",
            input_schema: schema(
                json!({
                    "type_name": {"type": "string", "description": "Validator type."},
                    "name": {"type": "string", "description": "File name without an extension."},
                    "directory": {"type": "string", "description": "Directory of JSON files. Defaults to tests/pcbcraft."}
                }),
                &[],
            ),
            call: tests_init_type,
        },
        Tool {
            name: "tests_single_run",
            description: "Run one test type from a spec object. Does not write a file. Exit code 1 is a failure and 2 is a schema or setup error.",
            input_schema: schema(
                json!({
                    "type_name": {"type": "string", "description": "Validator type, for example ERCValidator."},
                    "spec": {
                        "type": "object",
                        "description": "Test spec. The same object a test file stores under spec. project is relative to cwd.",
                        "additionalProperties": true
                    }
                }),
                &["type_name", "spec"],
            ),
            call: tests_single_run,
        },
        Tool {
            name: "parts_alternatives",
            description: "Search supplier catalogs from a YAML spec.",
            input_schema: schema(
                json!({
                    "spec": {"type": "string"},
                    "limit": {"type": "integer", "description": "Defaults to 10."},
                    "include_out_of_stock": {"type": "boolean"},
                    "basic": {"type": "boolean"},
                    "format": {"type": "string", "description": "text or yaml."}
                }),
                &["spec"],
            ),
            call: parts_alternatives,
        },
        Tool {
            name: "parts_search",
            description: "Search the JLCPCB assembly catalog by keyword and product attributes. Results are JSON.",
            input_schema: schema(
                json!({
                    "supplier": {"type": "string", "description": "jlcpcb"},
                    "query": {"type": "string", "description": "Keyword, for example resistor or STM32F103C8T6."},
                    "params": {"type": "string", "description": "JSON file of product attributes. Relative paths are resolved from cwd. The same object as attributes."},
                    "param": {
                        "type": "array",
                        "items": {"type": "string"},
                        "description": "Attributes as key=value, for example [\"Resistance=10kΩ\"]. Repeat a key to OR those values."
                    },
                    "attributes": {
                        "type": "object",
                        "description": "Product attributes. Each value is a string or a list of strings. package and manufacturer filter those fields. Any other key is a catalog attribute in the catalog's own spelling, for example {\"Resistance\":\"10kΩ\",\"Tolerance\":\"±1%\"}. A list is OR. Different keys are AND.",
                        "additionalProperties": {
                            "anyOf": [
                                {"type": "string"},
                                {"type": "array", "items": {"type": "string"}}
                            ]
                        }
                    },
                    "limit": {"type": "integer", "description": "Maximum hits. Defaults to 10. At most 50."},
                    "include_out_of_stock": {"type": "boolean"},
                    "basic": {"type": "boolean", "description": "Basic parts only."}
                }),
                &["supplier", "query"],
            ),
            call: parts_search,
        },
        Tool {
            name: "parts_download_model",
            description: "Download EasyEDA footprints and STEP models for LCSC numbers into a local KiCad library.",
            input_schema: schema(
                json!({
                    "lcsc": {
                        "type": "array",
                        "items": {"type": "string"},
                        "description": "LCSC numbers, for example [\"C2040\"]."
                    },
                    "library": {
                        "type": "string",
                        "description": "Directory that will contain EasyEDA.pretty and EasyEDA.3dshapes. Defaults to .pcbcraft under cwd."
                    },
                    "project": {
                        "type": "string",
                        "description": "KiCad project file. When set, its sibling fp-lib-table gains the EasyEDA library."
                    }
                }),
                &["lcsc"],
            ),
            call: parts_download_model,
        },
        Tool {
            name: "parts_download_datasheet",
            description: "Download an LCSC datasheet and write it as markdown with local images.",
            input_schema: schema(
                json!({
                    "supplier": {"type": "string", "description": "lcsc"},
                    "part": {"type": "string", "description": "LCSC number, for example C55266."},
                    "output_dir": {"type": "string", "description": "Directory for datasheet.md, datasheet.pdf, images, and extra. Relative paths are resolved from cwd."}
                }),
                &["supplier", "part", "output_dir"],
            ),
            call: parts_download_datasheet,
        },
        Tool {
            name: "components_sync",
            description: "Fetch LCSC and PCBWay cards for parts on the schematic.",
            input_schema: schema(
                json!({
                    "input": {"type": "string"},
                    "refresh": {"type": "boolean"}
                }),
                &[],
            ),
            call: components_sync,
        },
        Tool {
            name: "components_show",
            description: "Print one catalog card from the local database as JSON.",
            input_schema: schema(
                json!({
                    "part": {"type": "string", "description": "LCSC or PCBWay catalog number, for example C12345."},
                    "input": {"type": "string"}
                }),
                &["part"],
            ),
            call: components_show,
        },
        Tool {
            name: "net_dump",
            description: "Print the graph and pin list of one net.",
            input_schema: schema(
                json!({
                    "name": {"type": "string"},
                    "project": {"type": "string"}
                }),
                &["name"],
            ),
            call: net_dump,
        },
        Tool {
            name: "geometry_dump",
            description: "Print outlines on the whole board, or inside a window.",
            input_schema: schema(
                json!({
                    "window": {"type": "string", "description": "XMIN-XMAXxYMIN-YMAX, for example 0-50x0-40."},
                    "all": {"type": "boolean"},
                    "detailed": {"type": "boolean"},
                    "detailed_precision": {"type": "number", "description": "Sampling step in millimetres. Defaults to 0.01."},
                    "format": {"type": "string", "description": "json or png. Defaults to json."},
                    "output": {"type": "string", "description": "JSON defaults to stdout. PNG defaults to pcb-geometry.png."},
                    "board": {"type": "string"},
                    "input": {"type": "string"}
                }),
                &[],
            ),
            call: geometry_dump,
        },
        Tool {
            name: "geometry_is_occupied",
            description: "Report whether a point is inside a blocking area.",
            input_schema: schema(
                json!({
                    "x": {"type": "number"},
                    "y": {"type": "number"},
                    "size": {"type": "number", "description": "Side of the square around the point, in millimetres. Defaults to 10."},
                    "detailed": {"type": "boolean"},
                    "detailed_precision": {"type": "number", "description": "Defaults to 0.01."},
                    "layer": {"type": "array", "items": {"type": "string"}, "description": "Layers that count, for example [\"F.Cu\"]. Edge.Cuts and drills always count. Omit it to count every layer."},
                    "board": {"type": "string"},
                    "input": {"type": "string"}
                }),
                &["x", "y"],
            ),
            call: geometry_is_occupied,
        },
        Tool {
            name: "geometry_find_symbol",
            description: "Print the shapes of one footprint placed on the board.",
            input_schema: schema(
                json!({
                    "reference": {"type": "string", "description": "Footprint reference, for example U1."},
                    "board": {"type": "string"},
                    "input": {"type": "string"}
                }),
                &["reference"],
            ),
            call: geometry_find_symbol,
        },
    ]
}

fn board_setup(value: &Value) -> CommandOutput {
    invoke(value, |cwd, args: args::BoardSetupArgs| {
        board::setup(&cwd, &args)
    })
}

fn export_bom(value: &Value) -> CommandOutput {
    invoke(value, |cwd, args: args::FileArgs| export::bom(cwd, &args))
}

fn export_pos(value: &Value) -> CommandOutput {
    invoke(value, |cwd, args: args::FileArgs| export::pos(cwd, &args))
}

fn export_cpl(value: &Value) -> CommandOutput {
    invoke(value, |cwd, args: args::CplArgs| export::cpl(cwd, &args))
}

fn export_gerbers(value: &Value) -> CommandOutput {
    invoke(value, |cwd, args: args::FileArgs| {
        export::gerbers(cwd, &args)
    })
}

fn export_drawing(value: &Value) -> CommandOutput {
    invoke(value, |cwd, args: args::DrawingArgs| {
        export::drawing(cwd, &args)
    })
}

fn export_2d(value: &Value) -> CommandOutput {
    invoke(value, |cwd, args: args::TwoDArgs| export::two_d(cwd, &args))
}

fn tests_run(value: &Value) -> CommandOutput {
    invoke(value, |cwd, args: args::RunArgs| {
        tests_cmd::run(cwd, &args, false)
    })
}

fn tests_list(value: &Value) -> CommandOutput {
    invoke(value, |cwd, args: args::TestsDir| {
        tests_cmd::list(cwd, &args)
    })
}

fn tests_init_defaults(value: &Value) -> CommandOutput {
    invoke(value, |cwd, args: args::TestsDir| {
        tests_cmd::init_defaults(cwd, &args)
    })
}

fn tests_init_type(value: &Value) -> CommandOutput {
    invoke(value, |cwd, args: args::InitTypeArgs| {
        tests_cmd::init_type(cwd, &args)
    })
}

fn tests_single_run(value: &Value) -> CommandOutput {
    invoke(value, |cwd, args: args::SingleRunArgs| {
        tests_cmd::single_run(cwd, &args, false)
    })
}

fn parts_alternatives(value: &Value) -> CommandOutput {
    invoke(value, |cwd, args: args::AltArgs| {
        parts::alternatives(cwd, &args)
    })
}

fn parts_search(value: &Value) -> CommandOutput {
    invoke(value, |cwd, args: args::SearchArgs| {
        parts::search(cwd, &args)
    })
}

fn parts_download_model(value: &Value) -> CommandOutput {
    invoke(value, |cwd, args: args::DownloadArgs| {
        parts::download(cwd, &args, false)
    })
}

fn parts_download_datasheet(value: &Value) -> CommandOutput {
    invoke(value, |cwd, args: args::DatasheetArgs| {
        parts::download_datasheet(cwd, &args)
    })
}

fn components_sync(value: &Value) -> CommandOutput {
    invoke(value, |cwd, args: args::SyncArgs| {
        components::sync(cwd, &args)
    })
}

fn components_show(value: &Value) -> CommandOutput {
    invoke(value, |cwd, args: args::ShowArgs| {
        components::show(cwd, &args)
    })
}

fn net_dump(value: &Value) -> CommandOutput {
    invoke(value, |cwd, args: args::NetDumpArgs| net::dump(cwd, &args))
}

fn geometry_dump(value: &Value) -> CommandOutput {
    invoke(value, |cwd, args: DumpArgs| {
        crate::geometry_cmd::run(cwd, GeometryKind::Dump(args))
    })
}

fn geometry_is_occupied(value: &Value) -> CommandOutput {
    invoke(value, |cwd, args: OccupiedArgs| {
        crate::geometry_cmd::run(cwd, GeometryKind::IsOccupied(args))
    })
}

fn geometry_find_symbol(value: &Value) -> CommandOutput {
    invoke(value, |cwd, args: FindSymbolArgs| {
        crate::geometry_cmd::run(cwd, GeometryKind::FindSymbol(args))
    })
}

fn invoke<T, F>(value: &Value, run: F) -> CommandOutput
where
    T: DeserializeOwned,
    F: FnOnce(&Path, T) -> CommandOutput,
{
    let (cwd, rest) = match split_cwd(value) {
        Ok(pair) => pair,
        Err(output) => return output,
    };
    match serde_json::from_value(rest) {
        Ok(args) => run(&cwd, args),
        Err(err) => CommandOutput::error(2, format!("{err}\n")),
    }
}

fn split_cwd(value: &Value) -> Result<(PathBuf, Value), CommandOutput> {
    let Some(object) = value.as_object() else {
        return Err(CommandOutput::error(2, "arguments must be an object\n"));
    };
    let Some(cwd) = object.get("cwd") else {
        return Err(CommandOutput::error(2, "cwd is required\n"));
    };
    let Some(text) = cwd.as_str() else {
        return Err(CommandOutput::error(2, "cwd must be a string\n"));
    };
    let path = PathBuf::from(text);
    if !path.is_absolute() {
        return Err(CommandOutput::error(2, "cwd must be an absolute path\n"));
    }
    if !path.is_dir() {
        return Err(CommandOutput::error(
            2,
            format!("cwd is not a directory: {}\n", path.display()),
        ));
    }
    let mut rest = object.clone();
    rest.remove("cwd");
    Ok((path, Value::Object(rest)))
}

fn schema(mut properties: Value, extra_required: &[&str]) -> Value {
    properties.as_object_mut().expect("schema").insert(
        "cwd".into(),
        json!({
            "type": "string",
            "description": "Absolute project directory. Relative paths and pcbcraft.yaml are resolved from here."
        }),
    );
    let mut required = vec!["cwd".to_string()];
    required.extend(extra_required.iter().copied().map(str::to_string));
    json!({
        "type": "object",
        "properties": properties,
        "required": required,
        "additionalProperties": false
    })
}

#[cfg(test)]
mod tests {
    use super::tools;
    use serde_json::json;

    #[test]
    fn every_tool_has_a_description_and_tests_run_rejects_an_empty_directory() {
        let tools = tools();
        assert!(tools.iter().any(|tool| tool.name == "tests_run"));
        assert!(tools.iter().any(|tool| tool.name == "tests_single_run"));
        assert!(tools.iter().any(|tool| tool.name == "parts_search"));
        assert!(tools.iter().all(|tool| tool.name != "mcp"));
        for tool in tools {
            assert!(
                !tool.description.trim().is_empty(),
                "{} has an empty description",
                tool.name
            );
            assert!(tool.input_schema["required"]
                .as_array()
                .unwrap()
                .iter()
                .any(|item| item == "cwd"));
        }
        let dir = std::env::temp_dir().join(format!(
            "pcbcraft-mcp-empty-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let run = tools.iter().find(|tool| tool.name == "tests_run").unwrap();
        let output = (run.call)(&json!({
            "cwd": dir.display().to_string(),
            "directory": dir.display().to_string()
        }));
        assert_eq!(output.code, 2, "{}", output.stderr);
        assert!(output.stderr.contains("no JSON"), "{}", output.stderr);

        let missing = (run.call)(&json!({}));
        assert_eq!(missing.code, 2);
        assert!(missing.stderr.contains("cwd is required"));

        let relative = (run.call)(&json!({"cwd": "tests"}));
        assert_eq!(relative.code, 2);
        assert!(relative.stderr.contains("absolute"));

        let unknown = (run.call)(&json!({
            "cwd": dir.display().to_string(),
            "nope": true
        }));
        assert_eq!(unknown.code, 2, "{}", unknown.stderr);

        let search = tools
            .iter()
            .find(|tool| tool.name == "parts_search")
            .unwrap();
        let bad_supplier = (search.call)(&json!({
            "cwd": dir.display().to_string(),
            "supplier": "pcbway",
            "query": "resistor"
        }));
        assert_eq!(bad_supplier.code, 1, "{}", bad_supplier.stderr);
        assert!(
            bad_supplier.stderr.contains("jlcpcb"),
            "{}",
            bad_supplier.stderr
        );
        let bare = (search.call)(&json!({
            "cwd": dir.display().to_string(),
            "supplier": "jlcpcb",
            "query": "resistor",
            "attributes": {"Resistance": 10000}
        }));
        assert_eq!(bare.code, 1, "{}", bare.stderr);
        assert!(bare.stderr.contains("bare number"), "{}", bare.stderr);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn tests_single_run_rejects_a_spec_without_a_project_and_writes_nothing() {
        let tools = tools();
        let single = tools
            .iter()
            .find(|tool| tool.name == "tests_single_run")
            .unwrap();
        let required = single.input_schema["required"].as_array().unwrap();
        assert!(required.iter().any(|item| item == "cwd"));
        assert!(required.iter().any(|item| item == "type_name"));
        assert!(required.iter().any(|item| item == "spec"));
        assert_eq!(single.input_schema["additionalProperties"], false);

        let dir = std::env::temp_dir().join(format!(
            "pcbcraft-single-run-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let output = (single.call)(&json!({
            "cwd": dir.display().to_string(),
            "type_name": "ERCValidator",
            "spec": {}
        }));
        assert_eq!(output.code, 2, "{}{}", output.stdout, output.stderr);
        let text = format!("{}{}", output.stdout, output.stderr);
        assert!(text.contains("ERCValidator"), "{text}");
        assert!(std::fs::read_dir(&dir).unwrap().next().is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
