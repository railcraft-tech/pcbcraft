use std::collections::BTreeMap;

use pcbcraft_domain::geometry::{
    clip_outline, format_points, sample_outline, shape_meets_window, union_window, BoardGeometry,
    Curve, GeometryError, Outline, PathOutline, Point, Shape, Window,
};
use serde_json::{json, Map, Value};

#[derive(Debug, Clone, Copy)]
pub struct DumpOptions {
    pub all: bool,
    pub detailed: bool,
    pub precision: f64,
}

impl Default for DumpOptions {
    fn default() -> Self {
        Self {
            all: false,
            detailed: false,
            precision: 0.01,
        }
    }
}

/// Output is rounded to a nanometre, which hides float noise like 59.70000000022958.
pub(crate) fn round_mm(value: f64) -> f64 {
    let rounded = (value * 1e6).round() / 1e6;
    if rounded == 0.0 {
        0.0
    } else {
        rounded
    }
}

pub struct DumpView<'a> {
    pub window: Window,
    pub shapes: Vec<&'a Shape>,
}

/// Shapes and frame that `dump` would emit for these options.
pub fn dump_view<'a>(
    board: &'a BoardGeometry,
    window: Option<Window>,
    options: DumpOptions,
) -> Result<DumpView<'a>, GeometryError> {
    if options.detailed && options.precision <= 0.0 {
        return Err(GeometryError::BadPrecision);
    }
    let shapes = selected(board, window, options.all);
    let window = window.unwrap_or_else(|| union_window(&shapes));
    Ok(DumpView { window, shapes })
}

pub fn dump_json(
    board: &BoardGeometry,
    window: Option<Window>,
    options: DumpOptions,
) -> Result<Value, GeometryError> {
    let view = dump_view(board, window, options)?;
    match window {
        // A zone fill can have tens of thousands of edges, so a window gets only its part.
        Some(window) => {
            let owned = clipped(&view.shapes, window);
            let shapes: Vec<&Shape> = owned.iter().collect();
            view_json(
                &DumpView {
                    window: view.window,
                    shapes,
                },
                options,
            )
        }
        None => view_json(&view, options),
    }
}

/// Margin around a window kept when clipping, so tests just past its edge stay exact.
const CLIP_MARGIN_MM: f64 = 1e-3;

/// Copies of `shapes` cut down to `window`. The area inside the window is unchanged.
pub(crate) fn clipped(shapes: &[&Shape], window: Window) -> Vec<Shape> {
    let grown = Window {
        x_min: window.x_min - CLIP_MARGIN_MM,
        x_max: window.x_max + CLIP_MARGIN_MM,
        y_min: window.y_min - CLIP_MARGIN_MM,
        y_max: window.y_max + CLIP_MARGIN_MM,
    };
    let empty = || {
        Outline::Path(PathOutline {
            start: Point::new(grown.x_min, grown.y_min),
            segments: Vec::new(),
        })
    };
    shapes
        .iter()
        .filter_map(|shape| {
            let mut copy = (*shape).clone();
            let occupied = clip_outline(&shape.occupied, grown);
            if shape.inward {
                // Everything outside the outline blocks, so an empty cut still matters.
                copy.occupied = occupied.unwrap_or_else(empty);
                return Some(copy);
            }
            let clearance = clip_outline(&shape.clearance, grown);
            if occupied.is_none() && clearance.is_none() {
                return None;
            }
            copy.occupied = occupied.unwrap_or_else(empty);
            copy.clearance = clearance.unwrap_or_else(empty);
            Some(copy)
        })
        .collect()
}

/// JSON for a view that was already selected.
pub fn view_json(view: &DumpView<'_>, options: DumpOptions) -> Result<Value, GeometryError> {
    let mut layers: BTreeMap<String, Vec<Value>> = BTreeMap::new();
    for shape in &view.shapes {
        layers
            .entry(shape.layer.clone())
            .or_default()
            .push(shape_json(shape, options)?);
    }
    Ok(json!({
        "unit": "mm",
        "window": {
            "xMin": round_mm(view.window.x_min),
            "xMax": round_mm(view.window.x_max),
            "yMin": round_mm(view.window.y_min),
            "yMax": round_mm(view.window.y_max),
        },
        "layers": layers,
    }))
}

pub(crate) fn selected(board: &BoardGeometry, window: Option<Window>, all: bool) -> Vec<&Shape> {
    board
        .shapes
        .iter()
        .filter(|shape| {
            (all || shape.blocks) && window.is_none_or(|window| shape_meets_window(shape, window))
        })
        .collect()
}

fn shape_json(shape: &Shape, options: DumpOptions) -> Result<Value, GeometryError> {
    let mut object = Map::new();
    object.insert("source".into(), json!(shape.source.as_str()));
    if let Some(reference) = &shape.reference {
        object.insert("ref".into(), json!(reference));
    }
    if let Some(pad) = &shape.pad {
        object.insert("pad".into(), json!(pad));
    }
    if let Some(net) = &shape.net {
        object.insert("net".into(), json!(net));
    }
    if let Some(width) = shape.width {
        object.insert("width".into(), json!(round_mm(width)));
    }
    object.insert("occupied".into(), outline_json(&shape.occupied, options)?);
    if shape.blocks {
        object.insert("clearanceMm".into(), json!(round_mm(shape.clearance_mm)));
        object.insert("clearance".into(), outline_json(&shape.clearance, options)?);
        if shape.inward {
            object.insert("clearanceSide".into(), json!("inward"));
        }
    }
    Ok(Value::Object(object))
}

fn outline_json(outline: &Outline, options: DumpOptions) -> Result<Value, GeometryError> {
    let mut value = match outline {
        Outline::Circle { center, radius } => json!({
            "kind": "circle",
            "center": [round_mm(center.x), round_mm(center.y)],
            "radius": round_mm(*radius),
        }),
        Outline::Path(path) => {
            let segments: Vec<Value> = path
                .segments
                .iter()
                .map(|curve| match curve {
                    Curve::Line { to } => {
                        json!({"kind": "line", "to": [round_mm(to.x), round_mm(to.y)]})
                    }
                    Curve::Arc { to, mid } => {
                        json!({
                            "kind": "arc",
                            "to": [round_mm(to.x), round_mm(to.y)],
                            "mid": [round_mm(mid.x), round_mm(mid.y)],
                        })
                    }
                })
                .collect();
            json!({
                "kind": "filled",
                "start": [round_mm(path.start.x), round_mm(path.start.y)],
                "segments": segments,
            })
        }
    };
    if options.detailed {
        let points = format_points(
            &sample_outline(outline, options.precision),
            options.precision,
        );
        value
            .as_object_mut()
            .expect("outline object")
            .insert("points".into(), json!(points));
    }
    Ok(value)
}

/// Shapes that belong to one footprint, with a window around their occupied outlines.
pub fn symbol_view<'a>(
    board: &'a BoardGeometry,
    reference: &str,
) -> Result<(Window, Vec<&'a Shape>), GeometryError> {
    let shapes: Vec<&Shape> = board
        .shapes
        .iter()
        .filter(|shape| shape.reference.as_deref() == Some(reference))
        .collect();
    let window = pcbcraft_domain::geometry::occupied_window(&shapes)
        .ok_or_else(|| GeometryError::NoSymbol(reference.to_string()))?;
    Ok((window, shapes))
}

pub fn find_symbol(board: &BoardGeometry, reference: &str) -> Result<Value, GeometryError> {
    let (window, shapes) = symbol_view(board, reference)?;
    let mut layers: BTreeMap<String, Vec<Value>> = BTreeMap::new();
    for shape in shapes {
        layers
            .entry(shape.layer.clone())
            .or_default()
            .push(shape_json(shape, DumpOptions::default())?);
    }
    Ok(json!({
        "unit": "mm",
        "reference": reference,
        "window": {
            "xMin": round_mm(window.x_min),
            "xMax": round_mm(window.x_max),
            "yMin": round_mm(window.y_min),
            "yMax": round_mm(window.y_max),
        },
        "layers": layers,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pcbcraft_domain::geometry::{
        apply_clearance, stadium, BoardGeometry, ClearanceClass, ClearanceRules, NetClass, Point,
        Shape, Source,
    };

    use crate::geometry::occupied_json;
    use crate::geometry::OccupiedOptions;

    fn track_board() -> BoardGeometry {
        let mut board = BoardGeometry {
            copper_layers: vec!["F.Cu".into()],
            outer_copper_mm: None,
            inner_copper_mm: None,
            footprints: Vec::new(),
            shapes: vec![
                Shape {
                    source: Source::Track,
                    layer: "F.Cu".into(),
                    reference: None,
                    pad: None,
                    net: Some("GND".into()),
                    width: Some(0.25),
                    blocks: true,
                    clearance_class: ClearanceClass::Copper {
                        net: Some("GND".into()),
                    },
                    clearance_mm: 0.0,
                    mask_expansion_mm: 0.0,
                    drill_length_mm: None,
                    pad_form: None,
                    text_height_mm: None,
                    inset: false,
                    inward: false,
                    occupied: stadium(Point::new(0.0, 0.0), Point::new(10.0, 0.0), 0.25),
                    clearance: stadium(Point::new(0.0, 0.0), Point::new(10.0, 0.0), 0.25),
                },
                Shape {
                    source: Source::Graphic,
                    layer: "F.SilkS".into(),
                    reference: Some("R1".into()),
                    pad: None,
                    net: None,
                    width: Some(0.1),
                    blocks: false,
                    clearance_class: ClearanceClass::None,
                    clearance_mm: 0.0,
                    mask_expansion_mm: 0.0,
                    drill_length_mm: None,
                    pad_form: None,
                    text_height_mm: None,
                    inset: false,
                    inward: false,
                    occupied: stadium(Point::new(0.0, 2.0), Point::new(1.0, 2.0), 0.1),
                    clearance: stadium(Point::new(0.0, 2.0), Point::new(1.0, 2.0), 0.1),
                },
            ],
        };
        apply_clearance(
            &mut board,
            &ClearanceRules {
                min_clearance: 0.15,
                min_hole_clearance: 0.25,
                min_copper_edge_clearance: 0.5,
                classes: vec![NetClass {
                    name: "Default".into(),
                    clearance: 0.15,
                    priority: 1,
                }],
                patterns: Vec::new(),
            },
        );
        board
    }

    #[test]
    fn silk_is_hidden_until_all_and_points_need_detailed() {
        let board = track_board();
        let window = Window {
            x_min: 0.0,
            x_max: 10.0,
            y_min: -1.0,
            y_max: 3.0,
        };
        let plain = dump_json(&board, Some(window), DumpOptions::default()).unwrap();
        let text = plain.to_string();
        assert!(!text.contains("F.SilkS"));
        assert!(!text.contains("points"));
        let detailed = dump_json(
            &board,
            Some(window),
            DumpOptions {
                all: true,
                detailed: true,
                precision: 0.01,
            },
        )
        .unwrap();
        let text = detailed.to_string();
        assert!(text.contains("F.SilkS"));
        assert!(text.contains("points"));
        assert!(text.contains('x'));
    }

    #[test]
    fn a_point_in_the_clearance_is_occupied_toward_the_copper() {
        let board = track_board();
        let report = occupied_json(
            &board,
            Point::new(5.0, 0.2),
            OccupiedOptions {
                size: 10.0,
                ..OccupiedOptions::default()
            },
        )
        .unwrap();
        assert_eq!(report["occupied"], json!(true));
        assert_eq!(report["free"]["up"]["mm"], json!(0.0));
        let clear =
            occupied_json(&board, Point::new(5.0, 1.0), OccupiedOptions::default()).unwrap();
        assert_eq!(clear["occupied"], json!(false));
        let down = clear["free"]["up"]["mm"].as_f64().unwrap();
        assert!((down - 0.725).abs() < 1e-3, "{down}");
        assert_eq!(clear["free"]["up"]["source"], json!("track"));
        assert_eq!(clear["free"]["down"]["limitedByWindow"], json!(true));
        assert_eq!(clear["free"]["down"]["mm"], json!(5.0));
    }

    #[test]
    fn omitting_the_window_keeps_shapes_outside_a_small_span() {
        let board = track_board();
        let whole = dump_json(&board, None, DumpOptions::default()).unwrap();
        assert!(!whole["layers"]["F.Cu"].as_array().unwrap().is_empty());
        let cropped = dump_json(
            &board,
            Some(Window {
                x_min: 100.0,
                x_max: 110.0,
                y_min: 100.0,
                y_max: 110.0,
            }),
            DumpOptions::default(),
        )
        .unwrap();
        assert!(cropped["layers"].as_object().unwrap().is_empty());
    }

    #[test]
    fn find_symbol_keeps_one_footprint_and_ignores_its_clearance_halo() {
        let board = BoardGeometry {
            copper_layers: vec!["F.Cu".into()],
            outer_copper_mm: None,
            inner_copper_mm: None,
            footprints: Vec::new(),
            shapes: vec![
                Shape {
                    source: Source::Pad,
                    layer: "F.Cu".into(),
                    reference: Some("J1".into()),
                    pad: Some("1".into()),
                    net: None,
                    width: None,
                    blocks: true,
                    clearance_class: ClearanceClass::None,
                    clearance_mm: 1.0,
                    mask_expansion_mm: 0.0,
                    drill_length_mm: None,
                    pad_form: None,
                    text_height_mm: None,
                    inset: false,
                    inward: false,
                    occupied: Outline::Circle {
                        center: Point::new(5.0, 2.0),
                        radius: 1.0,
                    },
                    clearance: Outline::Circle {
                        center: Point::new(5.0, 2.0),
                        radius: 4.0,
                    },
                },
                Shape {
                    source: Source::Pad,
                    layer: "F.Cu".into(),
                    reference: Some("R1".into()),
                    pad: Some("1".into()),
                    net: None,
                    width: None,
                    blocks: true,
                    clearance_class: ClearanceClass::None,
                    clearance_mm: 0.0,
                    mask_expansion_mm: 0.0,
                    drill_length_mm: None,
                    pad_form: None,
                    text_height_mm: None,
                    inset: false,
                    inward: false,
                    occupied: Outline::Circle {
                        center: Point::new(0.0, 0.0),
                        radius: 1.0,
                    },
                    clearance: Outline::Circle {
                        center: Point::new(0.0, 0.0),
                        radius: 1.0,
                    },
                },
            ],
        };
        let value = find_symbol(&board, "J1").unwrap();
        assert_eq!(value["reference"], "J1");
        assert!((value["window"]["xMin"].as_f64().unwrap() - 4.0).abs() < 1e-9);
        assert!((value["window"]["xMax"].as_f64().unwrap() - 6.0).abs() < 1e-9);
        let pads = value["layers"]["F.Cu"].as_array().unwrap();
        assert_eq!(pads.len(), 1);
        assert_eq!(pads[0]["ref"], "J1");
        assert!(find_symbol(&board, "U1").is_err());
    }

    #[test]
    fn output_is_rounded_to_a_nanometre_and_layers_filter_occupancy() {
        assert_eq!(round_mm(59.70000000022958), 59.7);
        assert_eq!(round_mm(-1e-9), 0.0);
        let board = track_board();
        let at = Point::new(5.0, 0.2);
        let all = occupied_json(&board, at, OccupiedOptions::default()).unwrap();
        assert_eq!(all["occupied"], json!(true));
        let other = occupied_json(
            &board,
            at,
            OccupiedOptions {
                layers: Some(vec!["B.Cu".into()]),
                ..OccupiedOptions::default()
            },
        )
        .unwrap();
        assert_eq!(other["occupied"], json!(false));
        let same = occupied_json(
            &board,
            at,
            OccupiedOptions {
                layers: Some(vec!["F.Cu".into()]),
                ..OccupiedOptions::default()
            },
        )
        .unwrap();
        assert_eq!(same["occupied"], json!(true));
    }

    #[test]
    fn a_windowed_dump_keeps_only_the_window_part_of_a_big_zone() {
        let mut board = track_board();
        let ring: Vec<Point> = (0..400)
            .map(|index| {
                let angle = std::f64::consts::TAU * index as f64 / 400.0;
                Point::new(100.0 * angle.cos(), 100.0 * angle.sin())
            })
            .collect();
        let mut zone = board.shapes[0].clone();
        zone.source = Source::Zone;
        zone.width = None;
        zone.occupied = pcbcraft_domain::geometry::polygon(&ring);
        zone.clearance = zone.occupied.clone();
        board.shapes.push(zone);
        let window = Window {
            x_min: -5.0,
            x_max: 5.0,
            y_min: -5.0,
            y_max: 5.0,
        };
        let text = dump_json(&board, Some(window), DumpOptions::default())
            .unwrap()
            .to_string();
        assert!(text.len() < 5_000, "{}", text.len());
        let whole = dump_json(&board, None, DumpOptions::default())
            .unwrap()
            .to_string();
        assert!(whole.len() > 20_000);
        let report =
            occupied_json(&board, Point::new(0.0, 3.0), OccupiedOptions::default()).unwrap();
        assert_eq!(report["occupied"], json!(true));
    }
}
