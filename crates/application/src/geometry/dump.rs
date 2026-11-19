use std::collections::BTreeMap;

use pcbcraft_domain::geometry::{
    format_points, sample_outline, shape_meets_window, union_window, BoardGeometry, Curve,
    GeometryError, Outline, Shape, Window,
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
    let mut layers: BTreeMap<String, Vec<Value>> = BTreeMap::new();
    for shape in view.shapes {
        layers
            .entry(shape.layer.clone())
            .or_default()
            .push(shape_json(shape, options)?);
    }
    Ok(json!({
        "unit": "mm",
        "window": {
            "xMin": view.window.x_min,
            "xMax": view.window.x_max,
            "yMin": view.window.y_min,
            "yMax": view.window.y_max,
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
        object.insert("width".into(), json!(width));
    }
    object.insert("occupied".into(), outline_json(&shape.occupied, options)?);
    if shape.blocks {
        object.insert("clearanceMm".into(), json!(shape.clearance_mm));
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
            "center": [center.x, center.y],
            "radius": radius,
        }),
        Outline::Path(path) => {
            let segments: Vec<Value> = path
                .segments
                .iter()
                .map(|curve| match curve {
                    Curve::Line { to } => json!({"kind": "line", "to": [to.x, to.y]}),
                    Curve::Arc { to, mid } => {
                        json!({"kind": "arc", "to": [to.x, to.y], "mid": [mid.x, mid.y]})
                    }
                })
                .collect();
            json!({
                "kind": "filled",
                "start": [path.start.x, path.start.y],
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
            "xMin": window.x_min,
            "xMax": window.x_max,
            "yMin": window.y_min,
            "yMax": window.y_max,
        },
        "layers": layers,
    }))
}

/// Rebuild shapes from a full-board dump. `points` are ignored.
pub fn board_from_dump(value: &Value) -> Result<BoardGeometry, GeometryError> {
    let layers = value
        .get("layers")
        .and_then(Value::as_object)
        .ok_or(GeometryError::BadCache)?;
    let mut shapes = Vec::new();
    for (layer, items) in layers {
        let items = items.as_array().ok_or(GeometryError::BadCache)?;
        for item in items {
            shapes.push(shape_from_dump(layer, item)?);
        }
    }
    Ok(BoardGeometry {
        copper_layers: Vec::new(),
        shapes,
        outer_copper_mm: None,
        inner_copper_mm: None,
        footprints: Vec::new(),
    })
}

fn shape_from_dump(layer: &str, value: &Value) -> Result<Shape, GeometryError> {
    let source = value
        .get("source")
        .and_then(Value::as_str)
        .and_then(pcbcraft_domain::geometry::Source::parse)
        .ok_or(GeometryError::BadCache)?;
    let occupied = outline_from_dump(value.get("occupied").ok_or(GeometryError::BadCache)?)?;
    let inward = value.get("clearanceSide").and_then(Value::as_str) == Some("inward");
    let (blocks, clearance_mm, clearance) = if let Some(clearance) = value.get("clearance") {
        (
            true,
            value
                .get("clearanceMm")
                .and_then(Value::as_f64)
                .ok_or(GeometryError::BadCache)?,
            outline_from_dump(clearance)?,
        )
    } else {
        (false, 0.0, occupied.clone())
    };
    Ok(Shape {
        source,
        layer: layer.to_string(),
        reference: value.get("ref").and_then(Value::as_str).map(str::to_string),
        pad: value.get("pad").and_then(Value::as_str).map(str::to_string),
        net: value.get("net").and_then(Value::as_str).map(str::to_string),
        width: value.get("width").and_then(Value::as_f64),
        blocks,
        clearance_class: pcbcraft_domain::geometry::ClearanceClass::None,
        clearance_mm,
        mask_expansion_mm: 0.0,
        drill_length_mm: None,
        pad_form: None,
        text_height_mm: None,
        inset: inward,
        inward,
        occupied,
        clearance,
    })
}

fn outline_from_dump(value: &Value) -> Result<Outline, GeometryError> {
    match value.get("kind").and_then(Value::as_str) {
        Some("circle") => {
            let center = point_array(value.get("center").ok_or(GeometryError::BadCache)?)?;
            let radius = value
                .get("radius")
                .and_then(Value::as_f64)
                .ok_or(GeometryError::BadCache)?;
            Ok(Outline::Circle { center, radius })
        }
        Some("filled") => {
            let start = point_array(value.get("start").ok_or(GeometryError::BadCache)?)?;
            let raw = value
                .get("segments")
                .and_then(Value::as_array)
                .ok_or(GeometryError::BadCache)?;
            let mut segments = Vec::with_capacity(raw.len());
            for segment in raw {
                let kind = segment.get("kind").and_then(Value::as_str);
                let to = point_array(segment.get("to").ok_or(GeometryError::BadCache)?)?;
                segments.push(match kind {
                    Some("line") => Curve::Line { to },
                    Some("arc") => Curve::Arc {
                        to,
                        mid: point_array(segment.get("mid").ok_or(GeometryError::BadCache)?)?,
                    },
                    _ => return Err(GeometryError::BadCache),
                });
            }
            Ok(Outline::Path(pcbcraft_domain::geometry::PathOutline {
                start,
                segments,
            }))
        }
        _ => Err(GeometryError::BadCache),
    }
}

fn point_array(value: &Value) -> Result<pcbcraft_domain::geometry::Point, GeometryError> {
    let pair = value.as_array().ok_or(GeometryError::BadCache)?;
    if pair.len() != 2 {
        return Err(GeometryError::BadCache);
    }
    Ok(pcbcraft_domain::geometry::Point::new(
        pair[0].as_f64().ok_or(GeometryError::BadCache)?,
        pair[1].as_f64().ok_or(GeometryError::BadCache)?,
    ))
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
        assert_eq!(report["free"]["down"]["mm"], json!(0.0));
        let clear =
            occupied_json(&board, Point::new(5.0, 1.0), OccupiedOptions::default()).unwrap();
        assert_eq!(clear["occupied"], json!(false));
        let down = clear["free"]["down"]["mm"].as_f64().unwrap();
        assert!((down - 0.725).abs() < 1e-3, "{down}");
        assert_eq!(clear["free"]["down"]["source"], json!("track"));
        assert_eq!(clear["free"]["up"]["limitedByWindow"], json!(true));
        assert_eq!(clear["free"]["up"]["mm"], json!(5.0));
    }

    #[test]
    fn omitting_the_window_keeps_shapes_outside_a_small_span() {
        let board = track_board();
        let whole = dump_json(&board, None, DumpOptions::default()).unwrap();
        assert!(whole["layers"]["F.Cu"].as_array().unwrap().len() >= 1);
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
    fn a_full_dump_round_trips_and_still_filters_a_window() {
        let board = track_board();
        let dumped = dump_json(
            &board,
            None,
            DumpOptions {
                all: true,
                detailed: false,
                precision: 0.01,
            },
        )
        .unwrap();
        let restored = board_from_dump(&dumped).unwrap();
        let cropped = dump_json(
            &restored,
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
        let track = restored
            .shapes
            .iter()
            .find(|shape| shape.source == Source::Track)
            .unwrap();
        assert!((track.width.unwrap() - 0.25).abs() < 1e-9);
        assert!(pcbcraft_domain::geometry::contains(
            &track.clearance,
            Point::new(5.0, 0.2)
        ));
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
}
