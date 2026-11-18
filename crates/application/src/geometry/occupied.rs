use pcbcraft_domain::geometry::{
    point_blocked, ray_hits, BoardGeometry, GeometryError, Point, Shape, Window,
};
use serde_json::{json, Value};

use super::dump::{dump_json, selected, DumpOptions};

#[derive(Debug, Clone, Copy)]
pub struct OccupiedOptions {
    pub size: f64,
    pub detailed: bool,
    pub precision: f64,
}

impl Default for OccupiedOptions {
    fn default() -> Self {
        Self {
            size: 10.0,
            detailed: false,
            precision: 0.01,
        }
    }
}

pub fn occupied_json(
    board: &BoardGeometry,
    point: Point,
    options: OccupiedOptions,
) -> Result<Value, GeometryError> {
    let window = Window::square(point.x, point.y, options.size);
    let shapes = selected(board, Some(window), false);
    let occupied = shapes.iter().any(|shape| point_blocked(shape, point));
    let dump_options = DumpOptions {
        all: false,
        detailed: options.detailed,
        precision: options.precision,
    };
    let nearby = dump_json(board, Some(window), dump_options)?;
    Ok(json!({
        "unit": "mm",
        "point": { "x": point.x, "y": point.y },
        "window": nearby["window"].clone(),
        "occupied": occupied,
        "free": {
            "left": direction_json(&shapes, point, window, Point::new(-1.0, 0.0)),
            "right": direction_json(&shapes, point, window, Point::new(1.0, 0.0)),
            "up": direction_json(&shapes, point, window, Point::new(0.0, 1.0)),
            "down": direction_json(&shapes, point, window, Point::new(0.0, -1.0)),
        },
        "nearby": { "layers": nearby["layers"].clone() },
    }))
}

fn direction_json(shapes: &[&Shape], origin: Point, window: Window, direction: Point) -> Value {
    let step = 1e-4;
    let ahead = Point::new(origin.x + direction.x * step, origin.y + direction.y * step);
    let inside = shapes.iter().any(|shape| point_blocked(shape, origin));
    if inside && shapes.iter().any(|shape| point_blocked(shape, ahead)) {
        let shape = shapes
            .iter()
            .copied()
            .find(|shape| point_blocked(shape, origin));
        return hit_json(0.0, false, shape);
    }
    let limit = window_limit(origin, direction, window);
    let mut best: Option<(f64, &Shape)> = None;
    for shape in shapes {
        for t in ray_hits(&shape.clearance, origin, direction) {
            if t <= step || t > limit + 1e-7 {
                continue;
            }
            let before = point_blocked(
                shape,
                Point::new(
                    origin.x + direction.x * (t - step),
                    origin.y + direction.y * (t - step),
                ),
            );
            let after = point_blocked(
                shape,
                Point::new(
                    origin.x + direction.x * (t + step),
                    origin.y + direction.y * (t + step),
                ),
            );
            if !before && after && best.map(|(so_far, _)| t < so_far).unwrap_or(true) {
                best = Some((t, shape));
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
        "mm": mm,
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
