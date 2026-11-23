//! KiCad 10 footprint libraries written from an EasyEDA drawing.

use std::fs;
use std::path::Path;

use anyhow::{bail, Context, Result};
use pcbcraft_application::ports::{FootprintStore, SavedFootprint};
use pcbcraft_domain::easyeda::{Drill, LibraryFootprint, ModelPlacement, PadKind, PadShape, Poly};

use crate::net::{atom_at, children, head, parse_sexp, SExpr};

const FORMAT_VERSION: &str = "20260206";

pub fn write_footprint(
    pretty_dir: &Path,
    shapes_dir: &Path,
    footprint: &LibraryFootprint,
    placement: Option<&ModelPlacement>,
    step: Option<&[u8]>,
    model_uri: &str,
) -> Result<SavedFootprint> {
    fs::create_dir_all(pretty_dir).with_context(|| format!("creating {}", pretty_dir.display()))?;
    let footprint_path = pretty_dir.join(format!("{}.kicad_mod", footprint.name));
    let text = render_footprint(footprint, placement.filter(|_| step.is_some()), model_uri);
    fs::write(&footprint_path, text)
        .with_context(|| format!("writing {}", footprint_path.display()))?;
    let step_path = if let Some(bytes) = step {
        fs::create_dir_all(shapes_dir)
            .with_context(|| format!("creating {}", shapes_dir.display()))?;
        let path = shapes_dir.join(format!("{}.step", footprint.name));
        fs::write(&path, bytes).with_context(|| format!("writing {}", path.display()))?;
        Some(path)
    } else {
        None
    };
    Ok(SavedFootprint {
        footprint_path,
        step_path,
    })
}

pub fn render_footprint(
    footprint: &LibraryFootprint,
    placement: Option<&ModelPlacement>,
    model_uri: &str,
) -> String {
    let mut out = String::new();
    let mut ids = 1u32;
    let (_min_x, _max_x, min_y, max_y) = pad_bounds(footprint);
    let center_x =
        footprint.pads.iter().map(|pad| pad.at.0).sum::<f64>() / footprint.pads.len().max(1) as f64;

    out.push_str(&format!(
        "(footprint {}\n\t(version {FORMAT_VERSION})\n\t(generator \"pcbcraft\")\n\t(generator_version \"{}\")\n\t(layer \"F.Cu\")\n",
        quote(&footprint.name),
        env!("CARGO_PKG_VERSION"),
    ));
    if !footprint.description.is_empty() {
        out.push_str(&format!("\t(descr {})\n", quote(&footprint.description)));
    }
    property(
        &mut out,
        "Reference",
        "REF**",
        (center_x, min_y - 1.2),
        "F.SilkS",
        false,
    );
    property(
        &mut out,
        "Value",
        &footprint.name,
        (center_x, max_y + 1.2),
        "F.Fab",
        false,
    );
    property(&mut out, "LCSC", &footprint.lcsc, (0.0, 0.0), "F.Fab", true);
    out.push_str(if footprint.through_hole {
        "\t(attr through_hole)\n"
    } else {
        "\t(attr smd)\n"
    });
    out.push_str("\t(duplicate_pad_numbers_are_jumpers no)\n");

    for line in &footprint.lines {
        graphic(
            &mut out,
            &mut ids,
            &format!(
                "(fp_line\n\t\t(start {} {})\n\t\t(end {} {})\n\t\t(stroke\n\t\t\t(width {})\n\t\t\t(type solid)\n\t\t)\n\t\t(layer {})\n\t)",
                num(line.start.0),
                num(line.start.1),
                num(line.end.0),
                num(line.end.1),
                num(line.width),
                quote(line.layer.kicad()),
            ),
        );
    }
    for circle in &footprint.circles {
        graphic(
            &mut out,
            &mut ids,
            &format!(
                "(fp_circle\n\t\t(center {} {})\n\t\t(end {} {})\n\t\t(stroke\n\t\t\t(width {})\n\t\t\t(type solid)\n\t\t)\n\t\t(fill no)\n\t\t(layer {})\n\t)",
                num(circle.center.0),
                num(circle.center.1),
                num(circle.center.0 + circle.radius),
                num(circle.center.1),
                num(circle.width),
                quote(circle.layer.kicad()),
            ),
        );
    }
    for arc in &footprint.arcs {
        graphic(
            &mut out,
            &mut ids,
            &format!(
                "(fp_arc\n\t\t(start {} {})\n\t\t(mid {} {})\n\t\t(end {} {})\n\t\t(stroke\n\t\t\t(width {})\n\t\t\t(type solid)\n\t\t)\n\t\t(layer {})\n\t)",
                num(arc.start.0),
                num(arc.start.1),
                num(arc.mid.0),
                num(arc.mid.1),
                num(arc.end.0),
                num(arc.end.1),
                num(arc.width),
                quote(arc.layer.kicad()),
            ),
        );
    }
    for poly in &footprint.polys {
        if poly.filled {
            graphic(&mut out, &mut ids, &filled_poly(poly));
        } else {
            for (start, end) in outline(poly) {
                graphic(
                    &mut out,
                    &mut ids,
                    &format!(
                        "(fp_line\n\t\t(start {} {})\n\t\t(end {} {})\n\t\t(stroke\n\t\t\t(width 0.05)\n\t\t\t(type solid)\n\t\t)\n\t\t(layer {})\n\t)",
                        num(start.0),
                        num(start.1),
                        num(end.0),
                        num(end.1),
                        quote(poly.layer.kicad()),
                    ),
                );
            }
        }
    }
    for label in &footprint.texts {
        let justify = if label.mirror { "left mirror" } else { "left" };
        let hide = if label.hidden { "\t\t(hide yes)\n" } else { "" };
        graphic(
            &mut out,
            &mut ids,
            &format!(
                "(fp_text user {}\n\t\t(at {} {} {})\n\t\t(layer {})\n{hide}\t\t(effects\n\t\t\t(font\n\t\t\t\t(size {} {})\n\t\t\t\t(thickness {})\n\t\t\t)\n\t\t\t(justify {justify})\n\t\t)\n\t)",
                quote(&label.text),
                num(label.at.0),
                num(label.at.1),
                num(label.rotation),
                quote(label.layer.kicad()),
                num(label.size),
                num(label.size),
                num(label.thickness),
            ),
        );
    }
    for pad in &footprint.pads {
        out.push('\t');
        out.push_str(&render_pad(pad, &mut ids));
        out.push('\n');
    }
    out.push_str("\t(embedded_fonts no)\n");
    if let Some(placement) = placement {
        out.push_str(&format!(
            "\t(model {}\n\t\t(offset\n\t\t\t(xyz {} {} {})\n\t\t)\n\t\t(scale\n\t\t\t(xyz 1 1 1)\n\t\t)\n\t\t(rotate\n\t\t\t(xyz {} {} {})\n\t\t)\n\t)\n",
            quote(model_uri),
            num(placement.offset[0]),
            num(placement.offset[1]),
            num(placement.offset[2]),
            num(placement.rotation[0]),
            num(placement.rotation[1]),
            num(placement.rotation[2]),
        ));
    }
    out.push_str(")\n");
    out
}

pub fn register_library(table_path: &Path, uri: &str) -> Result<()> {
    let text = if table_path.exists() {
        fs::read_to_string(table_path)
            .with_context(|| format!("reading {}", table_path.display()))?
    } else {
        "(fp_lib_table\n  (version 7)\n)\n".to_string()
    };
    let mut expr = parse_sexp(&text).context("fp-lib-table is not a KiCad library table")?;
    if head(&expr) != Some("fp_lib_table") {
        bail!("fp-lib-table is not a KiCad library table");
    }
    let SExpr::List(items) = &mut expr else {
        bail!("fp-lib-table is not a KiCad library table");
    };
    let lib = library_node(uri);
    if let Some(existing) = items
        .iter_mut()
        .find(|item| lib_name(item) == Some("EasyEDA"))
    {
        *existing = lib;
    } else {
        items.push(lib);
    }
    if let Some(parent) = table_path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
        }
    }
    fs::write(table_path, format_table(&expr))
        .with_context(|| format!("writing {}", table_path.display()))?;
    Ok(())
}

fn library_node(uri: &str) -> SExpr {
    SExpr::List(vec![
        SExpr::Atom("lib".into()),
        pair("name", "EasyEDA"),
        pair("type", "KiCad"),
        pair("uri", uri),
        pair("options", ""),
        pair("descr", "Footprints downloaded from EasyEDA by pcbcraft"),
    ])
}

fn pair(name: &str, value: &str) -> SExpr {
    SExpr::List(vec![SExpr::Atom(name.into()), SExpr::Atom(value.into())])
}

fn lib_name(expr: &SExpr) -> Option<&str> {
    children(expr).iter().find_map(|child| {
        if head(child) == Some("name") {
            atom_at(child, 1)
        } else {
            None
        }
    })
}

fn format_table(expr: &SExpr) -> String {
    let SExpr::List(items) = expr else {
        return String::new();
    };
    let mut out = String::from("(fp_lib_table\n");
    for item in items.iter().skip(1) {
        out.push_str("  ");
        out.push_str(&compact(item));
        out.push('\n');
    }
    out.push_str(")\n");
    out
}

fn compact(expr: &SExpr) -> String {
    match expr {
        SExpr::Atom(text) => format_atom(text),
        SExpr::List(items) => {
            let body = items.iter().map(compact).collect::<Vec<_>>().join(" ");
            format!("({body})")
        }
    }
}

fn format_atom(text: &str) -> String {
    let bare = !text.is_empty()
        && text
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '.' | '+' | '-'));
    if bare {
        text.to_string()
    } else {
        quote(text)
    }
}

fn graphic(out: &mut String, ids: &mut u32, body: &str) {
    let id = uuid(*ids);
    *ids += 1;
    let body = body.trim_end_matches('\n').trim_end_matches(')');
    out.push('\t');
    out.push_str(body);
    out.push_str(&format!("\n\t\t(uuid \"{id}\")\n\t)\n"));
}

fn render_pad(pad: &pcbcraft_domain::easyeda::Pad, ids: &mut u32) -> String {
    let kind = match pad.kind {
        PadKind::Smd => "smd",
        PadKind::ThruHole => "thru_hole",
        PadKind::NpThruHole => "np_thru_hole",
    };
    let shape = match pad.shape {
        PadShape::Rect => "rect",
        PadShape::Oval => "oval",
        PadShape::Circle => "circle",
        PadShape::Custom => "custom",
    };
    let at = if pad.rotation == 0.0 {
        format!("(at {} {})", num(pad.at.0), num(pad.at.1))
    } else {
        format!(
            "(at {} {} {})",
            num(pad.at.0),
            num(pad.at.1),
            num(pad.rotation)
        )
    };
    let mut out = format!(
        "(pad {} {kind} {shape}\n\t\t{at}\n\t\t(size {} {})\n",
        quote(&pad.number),
        num(pad.size.0),
        num(pad.size.1),
    );
    if let Some(drill) = pad.drill {
        let text = match drill {
            Drill::Round(diameter) => format!("(drill {})", num(diameter)),
            Drill::Oval(x, y) => format!("(drill oval {} {})", num(x), num(y)),
        };
        out.push_str(&format!("\t\t{text}\n"));
    }
    let layers = pad
        .layers
        .iter()
        .map(|layer| quote(layer.kicad()))
        .collect::<Vec<_>>()
        .join(" ");
    out.push_str(&format!("\t\t(layers {layers})\n"));
    if pad.shape == PadShape::Custom {
        let points = pad
            .polygon
            .iter()
            .map(|(x, y)| format!("(xy {} {})", num(*x), num(*y)))
            .collect::<Vec<_>>()
            .join(" ");
        out.push_str(&format!(
            "\t\t(options\n\t\t\t(clearance outline)\n\t\t\t(anchor rect)\n\t\t)\n\t\t(primitives\n\t\t\t(gr_poly\n\t\t\t\t(pts\n\t\t\t\t\t{points}\n\t\t\t\t)\n\t\t\t\t(width 0)\n\t\t\t\t(fill yes)\n\t\t\t)\n\t\t)\n"
        ));
    }
    let id = uuid(*ids);
    *ids += 1;
    out.push_str(&format!("\t\t(uuid \"{id}\")\n\t)"));
    out
}

fn filled_poly(poly: &Poly) -> String {
    let points = poly
        .points
        .iter()
        .map(|(x, y)| format!("(xy {} {})", num(*x), num(*y)))
        .collect::<Vec<_>>()
        .join(" ");
    format!(
        "(fp_poly\n\t\t(pts\n\t\t\t{points}\n\t\t)\n\t\t(stroke\n\t\t\t(width 0)\n\t\t\t(type solid)\n\t\t)\n\t\t(fill solid)\n\t\t(layer {})\n\t)",
        quote(poly.layer.kicad()),
    )
}

fn outline(poly: &Poly) -> Vec<((f64, f64), (f64, f64))> {
    let mut points = poly.points.clone();
    if points.len() >= 2 && points.first() != points.last() {
        points.push(points[0]);
    }
    points.windows(2).map(|pair| (pair[0], pair[1])).collect()
}

fn property(out: &mut String, name: &str, value: &str, at: (f64, f64), layer: &str, hide: bool) {
    out.push_str(&format!(
        "\t(property {} {}\n\t\t(at {} {} 0)\n\t\t(layer {})\n",
        quote(name),
        quote(value),
        num(at.0),
        num(at.1),
        quote(layer),
    ));
    if hide {
        out.push_str("\t\t(hide yes)\n");
    }
    out.push_str(
        "\t\t(effects\n\t\t\t(font\n\t\t\t\t(size 1 1)\n\t\t\t\t(thickness 0.15)\n\t\t\t)\n\t\t)\n\t)\n",
    );
}

fn pad_bounds(footprint: &LibraryFootprint) -> (f64, f64, f64, f64) {
    let mut min_x = f64::MAX;
    let mut max_x = f64::MIN;
    let mut min_y = f64::MAX;
    let mut max_y = f64::MIN;
    for pad in &footprint.pads {
        let half_x = pad.size.0.abs() / 2.0;
        let half_y = pad.size.1.abs() / 2.0;
        min_x = min_x.min(pad.at.0 - half_x);
        max_x = max_x.max(pad.at.0 + half_x);
        min_y = min_y.min(pad.at.1 - half_y);
        max_y = max_y.max(pad.at.1 + half_y);
    }
    if footprint.pads.is_empty() {
        (0.0, 0.0, 0.0, 0.0)
    } else {
        (min_x, max_x, min_y, max_y)
    }
}

fn uuid(n: u32) -> String {
    format!("00000000-0000-4000-8000-{n:012x}")
}

fn num(value: f64) -> String {
    let value = if value.abs() < 5e-7 { 0.0 } else { value };
    let rounded = (value * 1_000_000.0).round() / 1_000_000.0;
    let mut text = format!("{rounded:.6}");
    if text.contains('.') {
        while text.ends_with('0') {
            text.pop();
        }
        if text.ends_with('.') {
            text.pop();
        }
    }
    if text == "-0" {
        "0".into()
    } else {
        text
    }
}

fn quote(text: &str) -> String {
    let mut out = String::from("\"");
    for ch in text.chars() {
        match ch {
            '\\' | '"' => {
                out.push('\\');
                out.push(ch);
            }
            '\n' => out.push_str("\\n"),
            _ => out.push(ch),
        }
    }
    out.push('"');
    out
}

#[derive(Debug, Default, Clone, Copy)]
pub struct EasyedaLibrary;

impl FootprintStore for EasyedaLibrary {
    fn save(
        &self,
        pretty_dir: &Path,
        shapes_dir: &Path,
        footprint: &LibraryFootprint,
        placement: Option<&ModelPlacement>,
        step: Option<&[u8]>,
        model_uri: &str,
    ) -> Result<SavedFootprint> {
        write_footprint(
            pretty_dir, shapes_dir, footprint, placement, step, model_uri,
        )
    }

    fn register(&self, table: &Path, uri: &str) -> Result<()> {
        register_library(table, uri)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pcbcraft_domain::easyeda::{convert, place_model};
    use serde_json::json;

    fn sample() -> LibraryFootprint {
        convert(
            "C2040",
            &json!({
                "description": "resistor",
                "packageDetail": {
                    "title": "R0805",
                    "dataStr": {
                        "head": {"x": 4000, "y": 3000},
                        "shape": [
                            "PAD~RECT~3990~3000~10~10~1~~1~0~~0~id~0~~Y~0",
                            "PAD~RECT~4010~3000~10~10~1~~2~0~~0~id~0~~Y~0",
                            "SVGNODE~{\"attrs\":{\"uuid\":\"abc\",\"c_origin\":\"4000,3000\",\"z\":\"0\",\"c_rotation\":\"0,0,0\"}}~"
                        ]
                    }
                }
            }),
        )
        .unwrap()
    }

    #[test]
    fn the_footprint_is_kicad_10_and_the_pads_sit_near_the_origin() {
        let footprint = sample();
        let placement = place_model(footprint.model.as_ref().unwrap(), None);
        let text = render_footprint(
            &footprint,
            Some(&placement),
            "${KIPRJMOD}/.pcbcraft/EasyEDA.3dshapes/R0805.step",
        );
        assert!(text.contains("(version 20260206)"));
        assert!(text.contains("(generator \"pcbcraft\")"));
        assert!(text.contains("(attr smd)"));
        assert!(text.contains("(pad \"1\" smd rect"));
        assert!(text.contains("(at -2.54 0)"));
        assert!(text.contains("(at 2.54 0)"));
        assert!(text.contains("(embedded_fonts no)"));
        assert!(text.contains("${KIPRJMOD}/.pcbcraft/EasyEDA.3dshapes/R0805.step"));
        parse_sexp(&text).expect("footprint sexp");
    }

    #[test]
    fn registering_twice_keeps_one_easyeda_library() {
        let dir = std::env::temp_dir().join(format!(
            "pcbcraft-fp-lib-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let table = dir.join("fp-lib-table");
        fs::write(
            &table,
            "(fp_lib_table\n  (version 7)\n  (lib (name \"Other\")(type \"KiCad\")(uri \"${KIPRJMOD}/Other.pretty\")(options \"\")(descr \"\"))\n)\n",
        )
        .unwrap();
        register_library(&table, "${KIPRJMOD}/.pcbcraft/EasyEDA.pretty").unwrap();
        register_library(&table, "${KIPRJMOD}/libs/EasyEDA.pretty").unwrap();
        let text = fs::read_to_string(&table).unwrap();
        assert_eq!(text.matches("(name EasyEDA)").count(), 1);
        assert!(text.contains("${KIPRJMOD}/libs/EasyEDA.pretty"));
        assert!(text.contains("(name Other)"));
        let _ = fs::remove_dir_all(&dir);
    }
}
