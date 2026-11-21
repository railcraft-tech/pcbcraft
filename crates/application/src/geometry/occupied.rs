use pcbcraft_domain::geometry::{
    point_blocked, ray_hits, BoardGeometry, GeometryError, Point, Shape, Source, Window,
};
use serde_json::{json, Value};

use super::dump::{clipped, dump_view, round_mm, view_json, DumpOptions, DumpView};

#[derive(Debug, Clone)]
pub struct OccupiedOptions {
    pub size: f64,
    pub detailed: bool,
    pub precision: f64,
    /// Only shapes on these layers count. Board outline and drills always do.
    pub layers: Option<Vec<String>>,
}

impl Default for OccupiedOptions {
    fn default() -> Self {
        Self {
            size: 10.0,
            detailed: false,
            precision: 0.01,
            layers: None,
        }
    }
}

pub fn occupied_json(
    board: &BoardGeometry,
    point: Point,
    options: OccupiedOptions,
) -> Result<Value, GeometryError> {
    let window = Window::square(point.x, point.y, options.size);
    if !options.size.is_finite() || options.size <= 0.0 {
        return Err(GeometryError::BadLength(options.size.to_string()));
    }
    let dump_options = DumpOptions {
        all: false,
        detailed: options.detailed,
        precision: options.precision,
    };
    let mut view = dump_view(board, Some(window), dump_options)?;
    if let Some(layers) = &options.layers {
        view.shapes.retain(|shape| {
            matches!(shape.source, Source::Edge | Source::Hole) || layers.contains(&shape.layer)
        });
    }
    // Only the window matters here, so big polygons are cut down to it first.
    let owned = clipped(&view.shapes, window);
    let shapes: Vec<&Shape> = owned.iter().collect();
    let nearby = view_json(
        &DumpView {
            window: view.window,
            shapes: shapes.clone(),
        },
        dump_options,
    )?;
    let shapes = &shapes;
    let inside = shapes
        .iter()
        .copied()
        .find(|shape| point_blocked(shape, point));
    let occupied = inside.is_some();
    Ok(json!({
        "unit": "mm",
        "point": { "x": round_mm(point.x), "y": round_mm(point.y) },
        "window": nearby["window"].clone(),
        "occupied": occupied,
        "free": {
            "left": direction_json(shapes, inside, point, window, Point::new(-1.0, 0.0)),
            "right": direction_json(shapes, inside, point, window, Point::new(1.0, 0.0)),
            // Y grows downward in the board file, so screen "up" is -Y.
            "up": direction_json(shapes, inside, point, window, Point::new(0.0, -1.0)),
            "down": direction_json(shapes, inside, point, window, Point::new(0.0, 1.0)),
        },
        "nearby": { "clipped": true, "layers": nearby["layers"].clone() },
    }))
}

fn direction_json(
    shapes: &[&Shape],
    inside: Option<&Shape>,
    origin: Point,
    window: Window,
    direction: Point,
) -> Value {
    let step = 1e-4;
    let ahead = Point::new(origin.x + direction.x * step, origin.y + direction.y * step);
    if inside.is_some() && shapes.iter().any(|shape| point_blocked(shape, ahead)) {
        return hit_json(0.0, false, inside);
    }
    let limit = window_limit(origin, direction, window);
    let mut best: Option<(f64, &Shape)> = None;
    for shape in shapes {
        let mut hits = ray_hits(&shape.clearance, origin, direction);
        hits.sort_by(f64::total_cmp);
        // Hits come nearest first, so the first that enters the shape is this shape's answer.
        // Anything past the best distance so far cannot win, so each shape stops early.
        for t in hits {
            if t <= step || t > limit + 1e-7 {
                continue;
            }
            if best.is_some_and(|(so_far, _)| t >= so_far) {
                break;
            }
            let at = |offset: f64| {
                Point::new(
                    origin.x + direction.x * (t + offset),
                    origin.y + direction.y * (t + offset),
                )
            };
            if !point_blocked(shape, at(-step)) && point_blocked(shape, at(step)) {
                best = Some((t, shape));
                break;
            }
        }
    }
    match best {
        Some((distance, shape)) => hit_json(distance, false, Some(shape)),
        None => hit_json(limit.max(0.0), true, None),
    }
}

fn hit_json(mm: f64, limited: bool, shape: Option<&Shape>) -> Value {
    let mut value = json!({
        "mm": round_mm(mm),
        "limitedByWindow": limited,
    });
    if let Some(shape) = shape {
        let object = value.as_object_mut().expect("object");
        object.insert("layer".into(), json!(shape.layer));
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
    }
    value
}

fn window_limit(origin: Point, direction: Point, window: Window) -> f64 {
    if direction.x < 0.0 {
        origin.x - window.x_min
    } else if direction.x > 0.0 {
        window.x_max - origin.x
    } else if direction.y > 0.0 {
        window.y_max - origin.y
    } else {
        origin.y - window.y_min
    }
}
