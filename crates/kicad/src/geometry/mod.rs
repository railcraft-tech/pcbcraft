//! Read copper, holes, courtyards, and keepouts from a `.kicad_pcb` file.

mod cache;

pub use cache::{cache_name, load_cached};

use std::collections::HashMap;

use anyhow::{bail, Context, Result};
use pcbcraft_domain::geometry::{
    polygon, rectangle, rotate_point, round_rect, stadium, BoardGeometry, ClearanceClass,
    ClearanceRules, Curve, Outline, PathOutline, Point, Shape, Source,
};
use serde_json::Value;

use crate::net::{atom_at, children, head, parse_sexp, SExpr};

pub fn parse_board(text: &str) -> Result<BoardGeometry> {
    let expr = parse_sexp(text).context("board s-expression")?;
    if head(&expr) != Some("kicad_pcb") {
        bail!("expected a kicad_pcb file");
    }
    let (copper, mask) = layers_of(&expr);
    let nets = net_table(&expr);
    let mask_expansion = board_mask_expansion(&expr);
    let mut shapes = Vec::new();
    let mut footprints = Vec::new();
    let mut edge_lines: Vec<(Point, Point)> = Vec::new();
    for item in children(&expr).iter().skip(1) {
        match head(item) {
            Some("footprint") => {
                footprint(
                    item,
                    &copper,
                    &mask,
                    &nets,
                    mask_expansion,
                    &mut shapes,
                    &mut footprints,
                    &mut edge_lines,
                )?;
            }
            Some("segment") => shapes.extend(track_segment(item, &nets)?),
            Some("arc") => shapes.extend(track_arc(item, &nets)?),
            Some("via") => shapes.extend(via_shapes(item, &copper, &nets)?),
            Some("zone") => shapes.extend(zone_shapes(item, &nets)?),
            Some(name) if name.starts_with("gr_") => {
                graphic(
                    item,
                    Place::identity(),
                    &mut shapes,
                    &mut edge_lines,
                    None,
                    true,
                )?;
            }
            _ => {}
        }
    }
    shapes.extend(chain_edge(edge_lines));
    let (outer_copper_mm, inner_copper_mm) = copper_thickness(&expr);
    Ok(BoardGeometry {
        copper_layers: copper,
        shapes,
        outer_copper_mm,
        inner_copper_mm,
        footprints,
    })
}

pub fn read_clearance_rules(text: &str) -> Result<ClearanceRules> {
    let value: Value = serde_json::from_str(text).context("project file")?;
    let rules = value
        .pointer("/board/design_settings/rules")
        .context("board design rules are missing")?;
    let min_clearance = rules
        .get("min_clearance")
        .and_then(Value::as_f64)
        .context("min_clearance is missing")?;
    let number =
        |key: &str, fallback: f64| rules.get(key).and_then(Value::as_f64).unwrap_or(fallback);
    let mut classes = Vec::new();
    if let Some(list) = value
        .pointer("/net_settings/classes")
        .and_then(Value::as_array)
    {
        for class in list {
            let Some(name) = class.get("name").and_then(Value::as_str) else {
                continue;
            };
            classes.push(pcbcraft_domain::geometry::NetClass {
                name: name.to_string(),
                clearance: class
                    .get("clearance")
                    .and_then(Value::as_f64)
                    .unwrap_or(min_clearance),
                priority: class.get("priority").and_then(Value::as_i64).unwrap_or(0),
            });
        }
    }
    let mut patterns = Vec::new();
    if let Some(list) = value
        .pointer("/net_settings/netclass_patterns")
        .and_then(Value::as_array)
    {
        for pattern in list {
            let (Some(class_name), Some(text)) = (
                pattern.get("netclass").and_then(Value::as_str),
                pattern.get("pattern").and_then(Value::as_str),
            ) else {
                continue;
            };
            patterns.push(pcbcraft_domain::geometry::NetPattern {
                class_name: class_name.to_string(),
                pattern: text.to_string(),
            });
        }
    }
    Ok(ClearanceRules {
        min_clearance,
        min_hole_clearance: number("min_hole_clearance", min_clearance),
        min_copper_edge_clearance: number("min_copper_edge_clearance", min_clearance),
        classes,
        patterns,
    })
}

#[derive(Clone, Copy)]
struct Place {
    at: Point,
    rot: f64,
    mirror: bool,
}

impl Place {
    fn identity() -> Self {
        Self {
            at: Point::new(0.0, 0.0),
            rot: 0.0,
            mirror: false,
        }
    }

    fn map_point(&self, local: Point) -> Point {
        let mut point = local;
        if self.mirror {
            point.x = -point.x;
        }
        // KiCad angles are counterclockwise with Y increasing downward.
        // rotate_point is counterclockwise with Y increasing upward, so the sign flips.
        let rotated = rotate_point(point, Point::new(0.0, 0.0), -self.rot);
        Point::new(rotated.x + self.at.x, rotated.y + self.at.y)
    }

    fn map_outline(&self, outline: Outline) -> Outline {
        match outline {
            Outline::Circle { center, radius } => Outline::Circle {
                center: self.map_point(center),
                radius,
            },
            Outline::Path(path) => Outline::Path(PathOutline {
                start: self.map_point(path.start),
                segments: path
                    .segments
                    .into_iter()
                    .map(|curve| match curve {
                        Curve::Line { to } => Curve::Line {
                            to: self.map_point(to),
                        },
                        Curve::Arc { to, mid } => Curve::Arc {
                            to: self.map_point(to),
                            mid: self.map_point(mid),
                        },
                    })
                    .collect(),
            }),
        }
    }
}

fn footprint(
    expr: &SExpr,
    copper: &[String],
    mask: &[String],
    nets: &HashMap<String, String>,
    mask_expansion: f64,
    shapes: &mut Vec<Shape>,
    footprints: &mut Vec<pcbcraft_domain::geometry::FootprintName>,
    edge_lines: &mut Vec<(Point, Point)>,
) -> Result<()> {
    // A footprint saved on the board already has its flip baked into pad coordinates.
    // Mirroring again would throw every back-side part across its origin.
    let _layer = child_atom(expr, "layer").unwrap_or("F.Cu");
    let mirror = false;
    let (at, rot) = at_pair(child(expr, "at")).unwrap_or((Point::new(0.0, 0.0), 0.0));
    let place = Place { at, rot, mirror };
    let reference = property(expr, "Reference");
    if let (Some(reference), Some(name)) = (reference.clone(), atom_at(expr, 1)) {
        footprints.push(pcbcraft_domain::geometry::FootprintName {
            reference,
            name: name.to_string(),
        });
    }
    for item in children(expr).iter().skip(1) {
        match head(item) {
            Some("pad") => shapes.extend(pad_shapes(
                item,
                &place,
                copper,
                mask,
                &reference,
                mask_expansion,
            )?),
            Some("property") => silk_property(item, place, &reference, shapes),
            Some("zone") => shapes.extend(zone_shapes(item, nets)?),
            Some(name) if name.starts_with("fp_") => {
                graphic(item, place, shapes, edge_lines, reference.as_deref(), false)?;
            }
            _ => {}
        }
    }
    Ok(())
}

fn pad_shapes(
    expr: &SExpr,
    place: &Place,
    copper: &[String],
    _mask: &[String],
    reference: &Option<String>,
    board_mask_expansion: f64,
) -> Result<Vec<Shape>> {
    let number = atom_at(expr, 1).unwrap_or("").to_string();
    let form = pad_form(atom_at(expr, 2).unwrap_or(""));
    let kind = atom_at(expr, 3).unwrap_or("");
    let (pad_at, pad_rot) = at_pair(child(expr, "at")).unwrap_or((Point::new(0.0, 0.0), 0.0));
    let (width, height) = size_of(expr).unwrap_or((0.0, 0.0));
    let locals = pad_outlines(expr, kind, width, height, reference, &number)?;
    let mut shapes = Vec::new();
    let net = net_of(expr, &HashMap::new());
    let layers = listed_copper(&layer_tokens(expr), copper);
    let expansion = child_number(expr, "solder_mask_margin").unwrap_or(board_mask_expansion);
    for local in locals {
        let placed = place_pad(local, pad_at, -pad_rot, place);
        for layer in &layers {
            let mut shape = make_shape(
                Source::Pad,
                layer.clone(),
                reference.clone(),
                Some(number.clone()),
                net.clone(),
                None,
                true,
                ClearanceClass::Copper { net: net.clone() },
                false,
                placed.clone(),
            );
            shape.pad_form = form;
            shape.mask_expansion_mm = expansion;
            shapes.push(shape);
        }
    }
    if let Some((drill, length)) = drill_size(expr) {
        let placed = place_pad(drill_outline(expr).expect("drill"), pad_at, -pad_rot, place);
        let mut shape = make_shape(
            Source::Hole,
            "drill".to_string(),
            reference.clone(),
            Some(number),
            net_of(expr, &HashMap::new()),
            Some(drill),
            true,
            ClearanceClass::Hole,
            false,
            placed,
        );
        shape.pad_form = form;
        shape.drill_length_mm = Some(length);
        shapes.push(shape);
    }
    Ok(shapes)
}

fn pad_form(kind: &str) -> Option<pcbcraft_domain::geometry::PadForm> {
    use pcbcraft_domain::geometry::PadForm;
    Some(match kind {
        "smd" | "connect" => PadForm::Smd,
        "thru_hole" => PadForm::Through,
        "np_thru_hole" => PadForm::NonPlated,
        _ => return None,
    })
}

fn drill_size(expr: &SExpr) -> Option<(f64, f64)> {
    let drill = child(expr, "drill")?;
    if atom_at(drill, 1) == Some("oval") {
        let width = atom_at(drill, 2)?.parse::<f64>().ok()?;
        let height = atom_at(drill, 3)?.parse::<f64>().ok()?;
        return Some((width.min(height), width.max(height)));
    }
    let diameter = atom_at(drill, 1)?.parse::<f64>().ok()?;
    Some((diameter, diameter))
}

fn pad_outlines(
    expr: &SExpr,
    kind: &str,
    width: f64,
    height: f64,
    reference: &Option<String>,
    number: &str,
) -> Result<Vec<Outline>> {
    let who = format!(
        "{} pad {number}",
        reference.as_deref().unwrap_or("footprint")
    );
    match kind {
        "circle" => Ok(vec![Outline::Circle {
            center: Point::new(0.0, 0.0),
            radius: width / 2.0,
        }]),
        "rect" => Ok(vec![rectangle(Point::new(0.0, 0.0), width, height)]),
        "oval" => Ok(vec![oval(width, height)]),
        "roundrect" => {
            let ratio = child(expr, "roundrect_rratio")
                .and_then(|item| atom_at(item, 1))
                .and_then(|text| text.parse().ok())
                .unwrap_or(0.0);
            let radius = ratio * width.min(height);
            Ok(vec![round_rect(
                Point::new(0.0, 0.0),
                width,
                height,
                radius,
                0.0,
            )])
        }
        "custom" => {
            let anchor = child(expr, "options")
                .and_then(|options| child(options, "anchor"))
                .and_then(|anchor| atom_at(anchor, 1))
                .unwrap_or("rect");
            let mut outlines = vec![match anchor {
                "circle" => Outline::Circle {
                    center: Point::new(0.0, 0.0),
                    radius: width.max(height) / 2.0,
                },
                _ => rectangle(Point::new(0.0, 0.0), width, height),
            }];
            outlines.extend(custom_primitives(expr)?);
            if outlines.is_empty() {
                bail!("{who} has a custom shape without primitives");
            }
            Ok(outlines)
        }
        other => bail!("unknown pad shape '{other}' on {who}"),
    }
}

fn custom_primitives(expr: &SExpr) -> Result<Vec<Outline>> {
    let Some(primitives) = child(expr, "primitives") else {
        return Ok(Vec::new());
    };
    let mut outlines = Vec::new();
    for item in children(primitives).iter().skip(1) {
        match head(item) {
            Some("gr_poly") => {
                if let Some(points) = pts_of(item) {
                    outlines.push(polygon(&points));
                }
            }
            Some("gr_line") => {
                if let (Some(start), Some(end)) = (xy_child(item, "start"), xy_child(item, "end")) {
                    let width = stroke_width(item).unwrap_or(0.0);
                    if width > 0.0 {
                        outlines.push(stadium(start, end, width));
                    }
                }
            }
            Some("gr_circle") => {
                if let (Some(center), Some(end)) = (xy_child(item, "center"), xy_child(item, "end"))
                {
                    outlines.push(Outline::Circle {
                        center,
                        radius: dist(center, end),
                    });
                }
            }
            Some(other) => bail!("unknown custom pad primitive '{other}'"),
            None => {}
        }
    }
    Ok(outlines)
}

fn drill_outline(expr: &SExpr) -> Option<Outline> {
    let drill = child(expr, "drill")?;
    if atom_at(drill, 1) == Some("oval") {
        let width = atom_at(drill, 2)?.parse().ok()?;
        let height = atom_at(drill, 3)?.parse().ok()?;
        return Some(oval(width, height));
    }
    let diameter = atom_at(drill, 1)?.parse::<f64>().ok()?;
    Some(Outline::Circle {
        center: Point::new(0.0, 0.0),
        radius: diameter / 2.0,
    })
}

fn place_pad(outline: Outline, at: Point, rot: f64, place: &Place) -> Outline {
    let rotated = rotate_outline_local(outline, rot);
    let translated = translate_outline(rotated, at);
    place.map_outline(translated)
}

fn via_shapes(
    expr: &SExpr,
    copper: &[String],
    nets: &HashMap<String, String>,
) -> Result<Vec<Shape>> {
    let at = xy_child(expr, "at").context("via position")?;
    let size = child_number(expr, "size").unwrap_or(0.0);
    let drill = child_number(expr, "drill").unwrap_or(0.0);
    let net = net_of(expr, nets);
    let mut shapes = Vec::new();
    let disk = Outline::Circle {
        center: at,
        radius: size / 2.0,
    };
    for layer in copper_span(&layer_tokens(expr), copper) {
        shapes.push(make_shape(
            Source::Via,
            layer,
            None,
            None,
            net.clone(),
            Some(size),
            true,
            ClearanceClass::Copper { net: net.clone() },
            false,
            disk.clone(),
        ));
    }
    if drill > 0.0 {
        let mut hole = make_shape(
            Source::Hole,
            "drill".to_string(),
            None,
            None,
            net,
            Some(drill),
            true,
            ClearanceClass::Hole,
            false,
            Outline::Circle {
                center: at,
                radius: drill / 2.0,
            },
        );
        hole.drill_length_mm = Some(drill);
        shapes.push(hole);
    }
    Ok(shapes)
}

fn track_segment(expr: &SExpr, nets: &HashMap<String, String>) -> Result<Vec<Shape>> {
    let start = xy_child(expr, "start").context("track start")?;
    let end = xy_child(expr, "end").context("track end")?;
    let width = child_number(expr, "width").unwrap_or(0.0);
    let layer = child_atom(expr, "layer").unwrap_or("F.Cu").to_string();
    let net = net_of(expr, nets);
    Ok(vec![make_shape(
        Source::Track,
        layer,
        None,
        None,
        net.clone(),
        Some(width),
        true,
        ClearanceClass::Copper { net },
        false,
        stadium(start, end, width),
    )])
}

fn track_arc(expr: &SExpr, nets: &HashMap<String, String>) -> Result<Vec<Shape>> {
    let start = xy_child(expr, "start").context("arc start")?;
    let mid = xy_child(expr, "mid").context("arc mid")?;
    let end = xy_child(expr, "end").context("arc end")?;
    let width = child_number(expr, "width").unwrap_or(0.0);
    let layer = child_atom(expr, "layer").unwrap_or("F.Cu").to_string();
    let net = net_of(expr, nets);
    let outline = arc_copper(start, mid, end, width).unwrap_or_else(|| stadium(start, end, width));
    Ok(vec![make_shape(
        Source::Track,
        layer,
        None,
        None,
        net.clone(),
        Some(width),
        true,
        ClearanceClass::Copper { net },
        false,
        outline,
    )])
}

fn zone_shapes(expr: &SExpr, nets: &HashMap<String, String>) -> Result<Vec<Shape>> {
    let net = net_of(expr, nets);
    let mut shapes = Vec::new();
    let blocking_keepout = keepout_blocks(expr);
    if blocking_keepout {
        if let Some(points) = pts_of(child(expr, "polygon").unwrap_or(expr)) {
            let layer = child_atom(expr, "layer").unwrap_or("F.Cu");
            shapes.push(make_shape(
                Source::Keepout,
                layer.to_string(),
                None,
                None,
                net.clone(),
                None,
                true,
                ClearanceClass::None,
                false,
                polygon(&points),
            ));
        }
    }
    for item in children(expr).iter().skip(1) {
        if head(item) != Some("filled_polygon") {
            continue;
        }
        let Some(points) = pts_of(item) else {
            continue;
        };
        let layer = child_atom(item, "layer")
            .or_else(|| child_atom(expr, "layer"))
            .unwrap_or("F.Cu");
        shapes.push(make_shape(
            Source::Zone,
            layer.to_string(),
            None,
            None,
            net.clone(),
            None,
            true,
            ClearanceClass::Copper { net: net.clone() },
            false,
            polygon(&points),
        ));
    }
    Ok(shapes)
}

fn keepout_blocks(expr: &SExpr) -> bool {
    let Some(keepout) = child(expr, "keepout") else {
        return false;
    };
    for item in children(keepout).iter().skip(1) {
        if atom_at(item, 1) == Some("not_allowed") {
            return true;
        }
    }
    false
}

fn graphic(
    expr: &SExpr,
    place: Place,
    shapes: &mut Vec<Shape>,
    edge_lines: &mut Vec<(Point, Point)>,
    reference: Option<&str>,
    board_level: bool,
) -> Result<()> {
    let layer = child_atom(expr, "layer").unwrap_or("").to_string();
    let width = stroke_width(expr).unwrap_or(0.0);
    let kind = head(expr).unwrap_or("");
    let courtyard = layer.contains("CrtYd");
    let edge = layer == "Edge.Cuts";
    let local = match kind {
        "fp_line" | "gr_line" => {
            let (Some(start), Some(end)) = (xy_child(expr, "start"), xy_child(expr, "end")) else {
                return Ok(());
            };
            let start = if board_level {
                start
            } else {
                place.map_point(start)
            };
            let end = if board_level {
                end
            } else {
                place.map_point(end)
            };
            if edge {
                edge_lines.push((start, end));
                return Ok(());
            }
            if courtyard {
                None
            } else {
                Some(stadium(start, end, width.max(0.0)))
            }
        }
        "fp_rect" | "gr_rect" => {
            let (Some(start), Some(end)) = (xy_child(expr, "start"), xy_child(expr, "end")) else {
                return Ok(());
            };
            let start = if board_level {
                start
            } else {
                place.map_point(start)
            };
            let end = if board_level {
                end
            } else {
                place.map_point(end)
            };
            let outline = rect_corners(start, end);
            if edge {
                shapes.push(make_shape(
                    Source::Edge,
                    layer,
                    None,
                    None,
                    None,
                    Some(width),
                    true,
                    ClearanceClass::Edge,
                    true,
                    outline,
                ));
                return Ok(());
            }
            let grown = if width > 0.0 {
                pcbcraft_domain::geometry::offset_outline(&outline, width / 2.0)
            } else {
                outline
            };
            Some(grown)
        }
        "fp_poly" | "gr_poly" => {
            let Some(mut points) = pts_of(expr) else {
                return Ok(());
            };
            if !board_level {
                points = points
                    .into_iter()
                    .map(|point| place.map_point(point))
                    .collect();
            }
            let outline = polygon(&points);
            if edge {
                shapes.push(make_shape(
                    Source::Edge,
                    layer,
                    None,
                    None,
                    None,
                    Some(width),
                    true,
                    ClearanceClass::Edge,
                    true,
                    outline,
                ));
                return Ok(());
            }
            Some(if width > 0.0 && !courtyard {
                pcbcraft_domain::geometry::offset_outline(&outline, width / 2.0)
            } else if courtyard && width > 0.0 {
                pcbcraft_domain::geometry::offset_outline(&outline, width / 2.0)
            } else {
                outline
            })
        }
        "fp_circle" | "gr_circle" => {
            let (Some(center), Some(end)) = (xy_child(expr, "center"), xy_child(expr, "end"))
            else {
                return Ok(());
            };
            let center = if board_level {
                center
            } else {
                place.map_point(center)
            };
            let end = if board_level {
                end
            } else {
                place.map_point(end)
            };
            let mut radius = dist(center, end);
            if width > 0.0 {
                radius += width / 2.0;
            }
            Some(Outline::Circle { center, radius })
        }
        "fp_arc" | "gr_arc" => {
            let (Some(start), Some(mid), Some(end)) = (
                xy_child(expr, "start"),
                xy_child(expr, "mid"),
                xy_child(expr, "end"),
            ) else {
                return Ok(());
            };
            let start = if board_level {
                start
            } else {
                place.map_point(start)
            };
            let _mid = if board_level {
                mid
            } else {
                place.map_point(mid)
            };
            let end = if board_level {
                end
            } else {
                place.map_point(end)
            };
            if edge {
                edge_lines.push((start, end));
                return Ok(());
            }
            Some(stadium(start, end, width.max(0.05)))
        }
        "fp_text" | "gr_text" => {
            let at = xy_child(expr, "at").unwrap_or(Point::new(0.0, 0.0));
            let at = if board_level { at } else { place.map_point(at) };
            let text = atom_at(expr, 1).unwrap_or("");
            let (sx, sy) = font_size(expr);
            Some(text_box(at, text, sx, sy))
        }
        "fp_curve" | "gr_curve" => {
            bail!("unsupported curve primitive on {layer}");
        }
        _ => return Ok(()),
    };
    let Some(outline) = local else {
        if courtyard {
            if let (Some(start), Some(end)) = (xy_child(expr, "start"), xy_child(expr, "end")) {
                if kind == "fp_line" || kind == "gr_line" {
                    let start = if board_level {
                        start
                    } else {
                        place.map_point(start)
                    };
                    let end = if board_level {
                        end
                    } else {
                        place.map_point(end)
                    };
                    edge_lines_like_courtyard(shapes, &layer, reference, start, end, width);
                }
            }
        }
        return Ok(());
    };
    let source = if courtyard {
        Source::Courtyard
    } else if edge {
        Source::Edge
    } else {
        Source::Graphic
    };
    let blocks = courtyard || edge;
    shapes.push(make_shape(
        source,
        layer,
        reference.map(str::to_string),
        None,
        None,
        if width > 0.0 { Some(width) } else { None },
        blocks,
        if edge {
            ClearanceClass::Edge
        } else {
            ClearanceClass::None
        },
        edge,
        outline,
    ));
    Ok(())
}

fn edge_lines_like_courtyard(
    shapes: &mut Vec<Shape>,
    layer: &str,
    reference: Option<&str>,
    start: Point,
    end: Point,
    width: f64,
) {
    shapes.push(make_shape(
        Source::Courtyard,
        layer.to_string(),
        reference.map(str::to_string),
        None,
        None,
        Some(width),
        true,
        ClearanceClass::None,
        false,
        stadium(start, end, width.max(0.05)),
    ));
}

fn chain_edge(lines: Vec<(Point, Point)>) -> Vec<Shape> {
    if lines.is_empty() {
        return Vec::new();
    }
    let mut unused = lines;
    let mut shapes = Vec::new();
    while !unused.is_empty() {
        let (start, mut end) = unused.remove(0);
        let mut points = vec![start, end];
        loop {
            let Some(index) = unused
                .iter()
                .position(|(a, b)| dist(*a, end) < 1e-4 || dist(*b, end) < 1e-4)
            else {
                break;
            };
            let (a, b) = unused.remove(index);
            end = if dist(a, end) < 1e-4 { b } else { a };
            points.push(end);
            if dist(end, start) < 1e-4 {
                break;
            }
        }
        let closed = points.len() >= 4 && dist(*points.last().unwrap(), start) < 1e-4;
        if closed {
            shapes.push(make_shape(
                Source::Edge,
                "Edge.Cuts".into(),
                None,
                None,
                None,
                None,
                true,
                ClearanceClass::Edge,
                true,
                polygon(&points),
            ));
        } else {
            for pair in points.windows(2) {
                shapes.push(make_shape(
                    Source::Edge,
                    "Edge.Cuts".into(),
                    None,
                    None,
                    None,
                    None,
                    true,
                    ClearanceClass::Edge,
                    false,
                    stadium(pair[0], pair[1], 0.05),
                ));
            }
        }
    }
    shapes
}

fn text_box(at: Point, text: &str, width: f64, height: f64) -> Outline {
    let box_width = (text.chars().count().max(1) as f64) * width * 0.6;
    rectangle(at, box_width, height.max(0.1))
}

fn rect_corners(start: Point, end: Point) -> Outline {
    polygon(&[
        start,
        Point::new(end.x, start.y),
        end,
        Point::new(start.x, end.y),
    ])
}

fn oval(width: f64, height: f64) -> Outline {
    if width >= height {
        let radius = height / 2.0;
        let reach = (width / 2.0 - radius).max(0.0);
        stadium(Point::new(-reach, 0.0), Point::new(reach, 0.0), height)
    } else {
        let radius = width / 2.0;
        let reach = (height / 2.0 - radius).max(0.0);
        stadium(Point::new(0.0, -reach), Point::new(0.0, reach), width)
    }
}

fn arc_copper(start: Point, mid: Point, end: Point, width: f64) -> Option<Outline> {
    if width <= 0.0 {
        return None;
    }
    Some(Outline::Path(PathOutline {
        start,
        segments: vec![
            Curve::Arc { to: end, mid },
            Curve::Arc {
                to: start,
                mid: {
                    let (center, radius) = circle_of(start, mid, end)?;
                    let outward_x = mid.x - center.x;
                    let outward_y = mid.y - center.y;
                    let length = (outward_x * outward_x + outward_y * outward_y).sqrt();
                    if length < 1e-9 {
                        return None;
                    }
                    Point::new(
                        center.x - outward_x / length * radius,
                        center.y - outward_y / length * radius,
                    )
                },
            },
        ],
    }))
}

fn circle_of(a: Point, b: Point, c: Point) -> Option<(Point, f64)> {
    let d = 2.0 * (a.x * (b.y - c.y) + b.x * (c.y - a.y) + c.x * (a.y - b.y));
    if d.abs() < 1e-12 {
        return None;
    }
    let a2 = a.x * a.x + a.y * a.y;
    let b2 = b.x * b.x + b.y * b.y;
    let c2 = c.x * c.x + c.y * c.y;
    let center = Point::new(
        (a2 * (b.y - c.y) + b2 * (c.y - a.y) + c2 * (a.y - b.y)) / d,
        (a2 * (c.x - b.x) + b2 * (a.x - c.x) + c2 * (b.x - a.x)) / d,
    );
    let dx = center.x - a.x;
    let dy = center.y - a.y;
    Some((center, (dx * dx + dy * dy).sqrt()))
}

fn make_shape(
    source: Source,
    layer: String,
    reference: Option<String>,
    pad: Option<String>,
    net: Option<String>,
    width: Option<f64>,
    blocks: bool,
    clearance_class: ClearanceClass,
    inset: bool,
    occupied: Outline,
) -> Shape {
    Shape {
        source,
        layer,
        reference,
        pad,
        net,
        width,
        blocks,
        clearance_class,
        clearance_mm: 0.0,
        mask_expansion_mm: 0.0,
        drill_length_mm: None,
        pad_form: None,
        text_height_mm: None,
        inset,
        inward: false,
        clearance: occupied.clone(),
        occupied,
    }
}

fn board_mask_expansion(expr: &SExpr) -> f64 {
    setup_number(expr, "pad_to_mask_clearance").unwrap_or(0.0)
}

fn copper_thickness(expr: &SExpr) -> (Option<f64>, Option<f64>) {
    let outer = layer_thickness(expr, "F.Cu").or_else(|| layer_thickness(expr, "B.Cu"));
    let inner = layer_thickness(expr, "In1.Cu").or_else(|| layer_thickness(expr, "In2.Cu"));
    (outer, inner)
}

fn layer_thickness(expr: &SExpr, name: &str) -> Option<f64> {
    fn walk(expr: &SExpr, name: &str) -> Option<f64> {
        if head(expr) == Some("layer") && atom_at(expr, 1) == Some(name) {
            return child_number(expr, "thickness");
        }
        for item in children(expr).iter().skip(1) {
            if let Some(found) = walk(item, name) {
                return Some(found);
            }
        }
        None
    }
    walk(expr, name)
}

fn setup_number(expr: &SExpr, name: &str) -> Option<f64> {
    fn walk(expr: &SExpr, name: &str) -> Option<f64> {
        if head(expr) == Some(name) {
            return atom_at(expr, 1)?.parse().ok();
        }
        for item in children(expr).iter().skip(1) {
            if let Some(found) = walk(item, name) {
                return Some(found);
            }
        }
        None
    }
    walk(expr, name)
}

fn silk_property(expr: &SExpr, place: Place, reference: &Option<String>, shapes: &mut Vec<Shape>) {
    let layer = child_atom(expr, "layer").unwrap_or("");
    if !layer.ends_with("SilkS") {
        return;
    }
    let at = xy_child(expr, "at").unwrap_or(Point::new(0.0, 0.0));
    let at = place.map_point(at);
    let text = atom_at(expr, 2).unwrap_or("");
    let (sx, sy) = font_size(expr);
    let mut shape = make_shape(
        Source::Graphic,
        layer.to_string(),
        reference.clone(),
        None,
        None,
        font_thickness(expr),
        false,
        ClearanceClass::None,
        false,
        text_box(at, text, sx, sy),
    );
    shape.text_height_mm = Some(sy.max(sx));
    shapes.push(shape);
}

fn font_thickness(expr: &SExpr) -> Option<f64> {
    let effects = child(expr, "effects")?;
    let font = child(effects, "font")?;
    child_number(font, "thickness").or_else(|| atom_at(child(font, "thickness")?, 1)?.parse().ok())
}

fn layers_of(expr: &SExpr) -> (Vec<String>, Vec<String>) {
    let mut copper = Vec::new();
    let mut mask = Vec::new();
    let Some(layers) = child(expr, "layers") else {
        return (copper, mask);
    };
    for item in children(layers).iter().skip(1) {
        let Some(name) = atom_at(item, 1) else {
            continue;
        };
        let kind = atom_at(item, 2).unwrap_or("");
        if kind == "signal" || kind == "power" || name.ends_with(".Cu") {
            copper.push(name.to_string());
        }
        if name.ends_with(".Mask") {
            mask.push(name.to_string());
        }
    }
    (copper, mask)
}

fn net_table(expr: &SExpr) -> HashMap<String, String> {
    let mut nets = HashMap::new();
    for item in children(expr).iter().skip(1) {
        if head(item) == Some("net") {
            if let (Some(id), Some(name)) = (atom_at(item, 1), atom_at(item, 2)) {
                nets.insert(id.to_string(), name.to_string());
            }
        }
    }
    nets
}

fn listed_copper(tokens: &[String], copper: &[String]) -> Vec<String> {
    if tokens.iter().any(|token| token == "*.Cu") {
        return copper.to_vec();
    }
    tokens
        .iter()
        .filter(|token| copper.iter().any(|name| name == *token))
        .cloned()
        .collect()
}

fn copper_span(tokens: &[String], copper: &[String]) -> Vec<String> {
    if tokens.iter().any(|token| token == "*.Cu") || tokens.is_empty() {
        return copper.to_vec();
    }
    let indexes: Vec<usize> = tokens
        .iter()
        .filter_map(|token| copper.iter().position(|name| name == token))
        .collect();
    if indexes.is_empty() {
        return Vec::new();
    }
    let min = *indexes.iter().min().unwrap();
    let max = *indexes.iter().max().unwrap();
    copper[min..=max].to_vec()
}

fn layer_tokens(expr: &SExpr) -> Vec<String> {
    child(expr, "layers")
        .map(|layers| {
            children(layers)
                .iter()
                .skip(1)
                .filter_map(|item| match item {
                    SExpr::Atom(text) => Some(text.clone()),
                    SExpr::List(_) => None,
                })
                .collect()
        })
        .unwrap_or_default()
}

fn net_of(expr: &SExpr, nets: &HashMap<String, String>) -> Option<String> {
    let net = child(expr, "net")?;
    let token = atom_at(net, 1)?;
    Some(
        nets.get(token)
            .cloned()
            .unwrap_or_else(|| token.to_string()),
    )
}

fn property(expr: &SExpr, name: &str) -> Option<String> {
    children(expr).iter().find_map(|item| {
        if head(item) == Some("property") && atom_at(item, 1) == Some(name) {
            atom_at(item, 2).map(str::to_string)
        } else {
            None
        }
    })
}

fn child<'a>(expr: &'a SExpr, name: &str) -> Option<&'a SExpr> {
    children(expr).iter().find(|item| head(item) == Some(name))
}

fn child_atom<'a>(expr: &'a SExpr, name: &str) -> Option<&'a str> {
    child(expr, name).and_then(|item| atom_at(item, 1))
}

fn child_number(expr: &SExpr, name: &str) -> Option<f64> {
    child_atom(expr, name)?.parse().ok()
}

fn xy_child(expr: &SExpr, name: &str) -> Option<Point> {
    let item = child(expr, name)?;
    Some(Point::new(
        atom_at(item, 1)?.parse().ok()?,
        atom_at(item, 2)?.parse().ok()?,
    ))
}

fn at_pair(item: Option<&SExpr>) -> Option<(Point, f64)> {
    let item = item?;
    let point = Point::new(
        atom_at(item, 1)?.parse().ok()?,
        atom_at(item, 2)?.parse().ok()?,
    );
    let rot = atom_at(item, 3)
        .and_then(|text| text.parse().ok())
        .unwrap_or(0.0);
    Some((point, rot))
}

fn size_of(expr: &SExpr) -> Option<(f64, f64)> {
    let size = child(expr, "size")?;
    Some((
        atom_at(size, 1)?.parse().ok()?,
        atom_at(size, 2)?.parse().ok()?,
    ))
}

fn pts_of(expr: &SExpr) -> Option<Vec<Point>> {
    let pts = child(expr, "pts")?;
    let mut points = Vec::new();
    for item in children(pts).iter().skip(1) {
        if head(item) == Some("xy") {
            if let (Some(x), Some(y)) = (atom_at(item, 1), atom_at(item, 2)) {
                if let (Ok(x), Ok(y)) = (x.parse::<f64>(), y.parse::<f64>()) {
                    points.push(Point::new(x, y));
                }
            }
        }
    }
    if points.is_empty() {
        None
    } else {
        Some(points)
    }
}

fn stroke_width(expr: &SExpr) -> Option<f64> {
    let stroke = child(expr, "stroke")?;
    child_number(stroke, "width").or_else(|| atom_at(child(expr, "width")?, 1)?.parse().ok())
}

fn font_size(expr: &SExpr) -> (f64, f64) {
    let Some(effects) = child(expr, "effects") else {
        return (1.0, 1.0);
    };
    let Some(font) = child(effects, "font") else {
        return (1.0, 1.0);
    };
    let Some(size) = child(font, "size") else {
        return (1.0, 1.0);
    };
    (
        atom_at(size, 1)
            .and_then(|text| text.parse().ok())
            .unwrap_or(1.0),
        atom_at(size, 2)
            .and_then(|text| text.parse().ok())
            .unwrap_or(1.0),
    )
}

fn rotate_outline_local(outline: Outline, rotation_deg: f64) -> Outline {
    translate_rotate(outline, Point::new(0.0, 0.0), rotation_deg)
}

fn translate_outline(outline: Outline, by: Point) -> Outline {
    match outline {
        Outline::Circle { center, radius } => Outline::Circle {
            center: Point::new(center.x + by.x, center.y + by.y),
            radius,
        },
        Outline::Path(path) => Outline::Path(PathOutline {
            start: Point::new(path.start.x + by.x, path.start.y + by.y),
            segments: path
                .segments
                .into_iter()
                .map(|curve| match curve {
                    Curve::Line { to } => Curve::Line {
                        to: Point::new(to.x + by.x, to.y + by.y),
                    },
                    Curve::Arc { to, mid } => Curve::Arc {
                        to: Point::new(to.x + by.x, to.y + by.y),
                        mid: Point::new(mid.x + by.x, mid.y + by.y),
                    },
                })
                .collect(),
        }),
    }
}

fn translate_rotate(outline: Outline, _origin: Point, rotation_deg: f64) -> Outline {
    match outline {
        Outline::Circle { center, radius } => Outline::Circle {
            center: rotate_point(center, Point::new(0.0, 0.0), rotation_deg),
            radius,
        },
        Outline::Path(path) => Outline::Path(PathOutline {
            start: rotate_point(path.start, Point::new(0.0, 0.0), rotation_deg),
            segments: path
                .segments
                .into_iter()
                .map(|curve| match curve {
                    Curve::Line { to } => Curve::Line {
                        to: rotate_point(to, Point::new(0.0, 0.0), rotation_deg),
                    },
                    Curve::Arc { to, mid } => Curve::Arc {
                        to: rotate_point(to, Point::new(0.0, 0.0), rotation_deg),
                        mid: rotate_point(mid, Point::new(0.0, 0.0), rotation_deg),
                    },
                })
                .collect(),
        }),
    }
}

fn dist(a: Point, b: Point) -> f64 {
    let dx = a.x - b.x;
    let dy = a.y - b.y;
    (dx * dx + dy * dy).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;
    use pcbcraft_domain::geometry::{
        apply_clearance, contains, parse_window, point_blocked, shape_meets_window,
    };

    const BOARD: &str = r#"
(kicad_pcb
  (layers
    (0 "F.Cu" signal)
    (2 "B.Cu" signal)
  )
  (net 1 "GND")
  (footprint "R" (layer "F.Cu") (at 10 10)
    (property "Reference" "R1" (at 0 0) (layer "F.SilkS"))
    (fp_rect (start -1 -0.5) (end 1 0.5) (stroke (width 0.05)) (layer "F.CrtYd"))
    (fp_line (start -0.4 0.6) (end 0.4 0.6) (stroke (width 0.1)) (layer "F.SilkS"))
    (pad "1" smd roundrect (at 0 0) (size 1 0.6) (layers "F.Cu") (roundrect_rratio 0.25) (net "GND"))
  )
  (footprint "R" (layer "B.Cu") (at 0 0)
    (property "Reference" "R2" (at 0 0) (layer "B.SilkS"))
    (pad "1" smd circle (at 1 0) (size 0.4 0.4) (layers "B.Cu") (net "GND"))
  )
  (segment (start 0 0) (end 10 0) (width 0.25) (layer "F.Cu") (net "GND"))
  (via (at 5 5) (size 0.6) (drill 0.3) (layers "F.Cu" "B.Cu") (net 1))
  (zone (net "GND") (layer "F.Cu")
    (polygon (pts (xy 0 0) (xy 1 0) (xy 1 1) (xy 0 1)))
    (filled_polygon (layer "F.Cu") (pts (xy 0 0) (xy 4 0) (xy 4 2) (xy 0 2))))
  (zone (layer "F.Cu") (name "ko")
    (keepout (tracks not_allowed) (vias allowed) (pads allowed) (copperpour allowed) (footprints allowed))
    (polygon (pts (xy 20 20) (xy 22 20) (xy 22 22) (xy 20 22))))
  (gr_line (start 0 0) (end 30 0) (stroke (width 0.05)) (layer "Edge.Cuts"))
  (gr_line (start 30 0) (end 30 20) (stroke (width 0.05)) (layer "Edge.Cuts"))
  (gr_line (start 30 20) (end 0 20) (stroke (width 0.05)) (layer "Edge.Cuts"))
  (gr_line (start 0 20) (end 0 0) (stroke (width 0.05)) (layer "Edge.Cuts"))
)
"#;

    fn loaded() -> BoardGeometry {
        let mut board = parse_board(BOARD).unwrap();
        apply_clearance(
            &mut board,
            &ClearanceRules {
                min_clearance: 0.15,
                min_hole_clearance: 0.25,
                min_copper_edge_clearance: 0.5,
                classes: vec![pcbcraft_domain::geometry::NetClass {
                    name: "Default".into(),
                    clearance: 0.15,
                    priority: 100,
                }],
                patterns: Vec::new(),
            },
        );
        board
    }

    #[test]
    fn back_pad_keeps_the_saved_x_and_silk_does_not_block() {
        let board = loaded();
        let pad = board
            .shapes
            .iter()
            .find(|shape| shape.reference.as_deref() == Some("R2") && shape.source == Source::Pad)
            .unwrap();
        let Outline::Circle { center, .. } = &pad.occupied else {
            panic!("circle");
        };
        // The board file already stores a flipped footprint's pad coordinates.
        assert!((center.x - 1.0).abs() < 1e-6);
        assert!(board.shapes.iter().any(|shape| {
            shape.layer == "F.SilkS" && !shape.blocks && shape.source == Source::Graphic
        }));
    }

    #[test]
    fn track_width_and_keepout_and_zone_fill() {
        let board = loaded();
        let track = board
            .shapes
            .iter()
            .find(|shape| shape.source == Source::Track)
            .unwrap();
        assert!(contains(&track.occupied, Point::new(5.0, 0.0)));
        assert!(!contains(&track.occupied, Point::new(5.0, 0.2)));
        assert!(contains(&track.clearance, Point::new(5.0, 0.2)));
        assert!((track.width.unwrap() - 0.25).abs() < 1e-9);
        let keepout = board
            .shapes
            .iter()
            .find(|shape| shape.source == Source::Keepout)
            .unwrap();
        assert!(keepout.blocks);
        assert!(contains(&keepout.occupied, Point::new(21.0, 21.0)));
        let zone = board
            .shapes
            .iter()
            .find(|shape| shape.source == Source::Zone)
            .unwrap();
        assert!(contains(&zone.occupied, Point::new(3.5, 1.5)));
        let window = parse_window("1-2x2.05-3").unwrap();
        assert!(shape_meets_window(zone, window));
    }

    #[test]
    fn courtyard_hole_and_unknown_pad() {
        let board = loaded();
        let yard = board
            .shapes
            .iter()
            .find(|shape| shape.source == Source::Courtyard)
            .unwrap();
        assert!(contains(&yard.occupied, Point::new(10.0, 10.0)));
        assert_eq!(yard.reference.as_deref(), Some("R1"));
        let hole = board
            .shapes
            .iter()
            .find(|shape| shape.source == Source::Hole)
            .unwrap();
        assert_eq!(hole.layer, "drill");
        assert!((hole.clearance_mm - 0.25).abs() < 1e-9);
        let edge = board
            .shapes
            .iter()
            .find(|shape| shape.source == Source::Edge && shape.inward)
            .unwrap();
        assert!(!point_blocked(edge, Point::new(15.0, 10.0)));
        assert!(point_blocked(edge, Point::new(0.2, 10.0)));
        let err = parse_board(
            r#"(kicad_pcb (layers (0 "F.Cu" signal)) (footprint "R" (layer "F.Cu") (at 0 0) (pad "1" smd trapezoid (at 0 0) (size 1 1) (layers "F.Cu"))))"#,
        );
        assert!(err.unwrap_err().to_string().contains("trapezoid"));
    }
}
