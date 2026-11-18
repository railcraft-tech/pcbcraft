//! Board outlines in pcbnew coordinates: millimetres, Y up.

use thiserror::Error;

const EPS: f64 = 1e-9;

#[derive(Debug, Error, PartialEq)]
pub enum GeometryError {
    #[error("clearance rules are missing")]
    MissingRules,
    #[error("detailed precision must be positive")]
    BadPrecision,
    #[error("window '{0}' is not XMIN-XMAXxYMIN-YMAX")]
    BadWindow(String),
    #[error("cached geometry cannot be read")]
    BadCache,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

impl Point {
    pub fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Curve {
    Line { to: Point },
    Arc { to: Point, mid: Point },
}

#[derive(Debug, Clone, PartialEq)]
pub struct PathOutline {
    pub start: Point,
    pub segments: Vec<Curve>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Outline {
    Path(PathOutline),
    Circle { center: Point, radius: f64 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Track,
    Pad,
    Via,
    Zone,
    Keepout,
    Courtyard,
    Hole,
    Edge,
    Graphic,
}

impl Source {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Track => "track",
            Self::Pad => "pad",
            Self::Via => "via",
            Self::Zone => "zone",
            Self::Keepout => "keepout",
            Self::Courtyard => "courtyard",
            Self::Hole => "hole",
            Self::Edge => "edge",
            Self::Graphic => "graphic",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        Some(match text {
            "track" => Self::Track,
            "pad" => Self::Pad,
            "via" => Self::Via,
            "zone" => Self::Zone,
            "keepout" => Self::Keepout,
            "courtyard" => Self::Courtyard,
            "hole" => Self::Hole,
            "edge" => Self::Edge,
            "graphic" => Self::Graphic,
            _ => return None,
        })
    }
}

/// Which KiCad rule grows the occupied area.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClearanceClass {
    Copper {
        net: Option<String>,
    },
    Hole,
    /// Closed board outline. The clearance is an inset, not an outward halo.
    Edge,
    None,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Shape {
    pub source: Source,
    pub layer: String,
    pub reference: Option<String>,
    pub pad: Option<String>,
    pub net: Option<String>,
    pub width: Option<f64>,
    pub blocks: bool,
    pub clearance_class: ClearanceClass,
    pub clearance_mm: f64,
    /// Closed board outline: clearance is inset and blocks outside that inset.
    pub inset: bool,
    /// Filled by [`apply_clearance`] when `inset` is set.
    pub inward: bool,
    pub occupied: Outline,
    pub clearance: Outline,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BoardGeometry {
    pub copper_layers: Vec<String>,
    pub shapes: Vec<Shape>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Window {
    pub x_min: f64,
    pub x_max: f64,
    pub y_min: f64,
    pub y_max: f64,
}

impl Window {
    pub fn square(x: f64, y: f64, side: f64) -> Self {
        let half = side / 2.0;
        Self {
            x_min: x - half,
            x_max: x + half,
            y_min: y - half,
            y_max: y + half,
        }
    }

    pub fn contains_x(&self, x: f64) -> bool {
        x >= self.x_min - EPS && x <= self.x_max + EPS
    }

    pub fn contains_y(&self, y: f64) -> bool {
        y >= self.y_min - EPS && y <= self.y_max + EPS
    }
}

#[derive(Debug, Clone)]
pub struct NetClass {
    pub name: String,
    pub clearance: f64,
    pub priority: i64,
}

#[derive(Debug, Clone)]
pub struct NetPattern {
    pub class_name: String,
    pub pattern: String,
}

#[derive(Debug, Clone)]
pub struct ClearanceRules {
    pub min_clearance: f64,
    pub min_hole_clearance: f64,
    pub min_copper_edge_clearance: f64,
    pub classes: Vec<NetClass>,
    pub patterns: Vec<NetPattern>,
}

impl ClearanceRules {
    pub fn for_net(&self, net: Option<&str>) -> f64 {
        let Some(net) = net else {
            return self.fallback();
        };
        let mut best: Option<&NetClass> = None;
        for pattern in &self.patterns {
            if !glob_match(&pattern.pattern, net) {
                continue;
            }
            let Some(class) = self
                .classes
                .iter()
                .find(|class| class.name == pattern.class_name)
            else {
                continue;
            };
            let replace = best
                .map(|current| class.priority < current.priority)
                .unwrap_or(true);
            if replace {
                best = Some(class);
            }
        }
        best.map(|class| class.clearance)
            .unwrap_or_else(|| self.fallback())
    }

    fn fallback(&self) -> f64 {
        self.classes
            .iter()
            .find(|class| class.name == "Default")
            .map(|class| class.clearance)
            .unwrap_or(self.min_clearance)
    }
}

pub fn apply_clearance(board: &mut BoardGeometry, rules: &ClearanceRules) {
    for shape in &mut board.shapes {
        let distance = match &shape.clearance_class {
            ClearanceClass::Copper { net } => rules.for_net(net.as_deref()),
            ClearanceClass::Hole => rules.min_hole_clearance,
            ClearanceClass::Edge => rules.min_copper_edge_clearance,
            ClearanceClass::None => 0.0,
        };
        shape.clearance_mm = distance;
        if distance.abs() < EPS || matches!(shape.clearance_class, ClearanceClass::None) {
            shape.clearance_mm = 0.0;
            shape.inward = false;
            shape.clearance = shape.occupied.clone();
            continue;
        }
        if shape.inset {
            shape.inward = true;
            shape.clearance = offset_outline(&shape.occupied, -distance);
        } else {
            shape.inward = false;
            shape.clearance = offset_outline(&shape.occupied, distance);
        }
    }
}

pub fn parse_window(text: &str) -> Result<Window, GeometryError> {
    let Some((x_part, y_part)) = text.split_once(['x', 'X']) else {
        return Err(GeometryError::BadWindow(text.into()));
    };
    let (x0, x1) = parse_range(x_part).map_err(|_| GeometryError::BadWindow(text.into()))?;
    let (y0, y1) = parse_range(y_part).map_err(|_| GeometryError::BadWindow(text.into()))?;
    Ok(Window {
        x_min: x0.min(x1),
        x_max: x0.max(x1),
        y_min: y0.min(y1),
        y_max: y0.max(y1),
    })
}

pub fn shape_meets_window(shape: &Shape, window: Window) -> bool {
    bounds_meet(outline_bounds(&shape.occupied), window)
        || bounds_meet(outline_bounds(&shape.clearance), window)
}

/// Smallest window that contains every occupied and clearance outline.
pub fn union_window(shapes: &[&Shape]) -> Window {
    let mut bounds: Option<Bounds> = None;
    for shape in shapes {
        for outline in [&shape.occupied, &shape.clearance] {
            let next = outline_bounds(outline);
            bounds = Some(match bounds {
                Some(mut so_far) => {
                    so_far.include(Point::new(next.min_x, next.min_y));
                    so_far.include(Point::new(next.max_x, next.max_y));
                    so_far
                }
                None => next,
            });
        }
    }
    match bounds {
        Some(bounds) => Window {
            x_min: bounds.min_x,
            x_max: bounds.max_x,
            y_min: bounds.min_y,
            y_max: bounds.max_y,
        },
        None => Window {
            x_min: 0.0,
            x_max: 0.0,
            y_min: 0.0,
            y_max: 0.0,
        },
    }
}

pub fn point_blocked(shape: &Shape, point: Point) -> bool {
    if !shape.blocks {
        return false;
    }
    if shape.inward {
        !contains(&shape.clearance, point)
    } else {
        contains(&shape.clearance, point)
    }
}

pub fn sample_outline(outline: &Outline, tolerance: f64) -> Vec<Point> {
    match outline {
        Outline::Circle { center, radius } => sample_circle(*center, *radius, tolerance),
        Outline::Path(path) => sample_path(path, tolerance),
    }
}

pub fn format_points(points: &[Point], step: f64) -> Vec<String> {
    let mut out = Vec::new();
    for point in points {
        let text = format!(
            "{}x{}",
            format_coord(point.x, step),
            format_coord(point.y, step)
        );
        if out.last() != Some(&text) {
            out.push(text);
        }
    }
    out
}

pub fn offset_outline(outline: &Outline, distance: f64) -> Outline {
    if distance.abs() < EPS {
        return outline.clone();
    }
    match outline {
        Outline::Circle { center, radius } => Outline::Circle {
            center: *center,
            radius: (radius + distance).max(0.0),
        },
        Outline::Path(path) => Outline::Path(offset_path(path, distance)),
    }
}

pub fn contains(outline: &Outline, point: Point) -> bool {
    match outline {
        Outline::Circle { center, radius } => distance(point, *center) <= *radius + 1e-7,
        Outline::Path(path) => point_in_path(path, point),
    }
}

/// Positive distances where the ray `origin + t * direction` meets the outline.
pub fn ray_hits(outline: &Outline, origin: Point, direction: Point) -> Vec<f64> {
    match outline {
        Outline::Circle { center, radius } => ray_circle(origin, direction, *center, *radius),
        Outline::Path(path) => {
            let mut hits = Vec::new();
            let mut cursor = path.start;
            for curve in &path.segments {
                match curve {
                    Curve::Line { to } => {
                        if let Some(t) = ray_segment(origin, direction, cursor, *to) {
                            hits.push(t);
                        }
                        cursor = *to;
                    }
                    Curve::Arc { to, mid } => {
                        if let Some((center, radius)) = circle_through(cursor, *mid, *to) {
                            for t in ray_circle(origin, direction, center, radius) {
                                let at = add(origin, scale(direction, t));
                                if on_arc(cursor, *mid, *to, center, at) {
                                    hits.push(t);
                                }
                            }
                        }
                        cursor = *to;
                    }
                }
            }
            hits
        }
    }
}

pub fn stadium(start: Point, end: Point, width: f64) -> Outline {
    let radius = width / 2.0;
    if distance(start, end) < EPS {
        return Outline::Circle {
            center: start,
            radius,
        };
    }
    let direction = norm(sub(end, start));
    let side = Point::new(direction.y, -direction.x);
    let left_start = add(start, scale(side, radius));
    let left_end = add(end, scale(side, radius));
    let right_end = add(end, scale(side, -radius));
    let right_start = add(start, scale(side, -radius));
    Outline::Path(PathOutline {
        start: left_start,
        segments: vec![
            Curve::Line { to: left_end },
            Curve::Arc {
                to: right_end,
                mid: add(end, scale(direction, radius)),
            },
            Curve::Line { to: right_start },
            Curve::Arc {
                to: left_start,
                mid: add(start, scale(direction, -radius)),
            },
        ],
    })
}

pub fn round_rect(
    center: Point,
    width: f64,
    height: f64,
    radius: f64,
    rotation_deg: f64,
) -> Outline {
    let radius = radius.clamp(0.0, width.min(height) / 2.0);
    let hx = width / 2.0;
    let hy = height / 2.0;
    let local = if radius < EPS {
        rectangle(Point::new(0.0, 0.0), width, height)
    } else {
        let arc = |cx: f64, cy: f64, a0: f64, a1: f64| -> Curve {
            let mid = a0 + (a1 - a0) / 2.0;
            Curve::Arc {
                to: Point::new(cx + radius * a1.cos(), cy + radius * a1.sin()),
                mid: Point::new(cx + radius * mid.cos(), cy + radius * mid.sin()),
            }
        };
        let start = Point::new(-hx + radius, -hy);
        Outline::Path(PathOutline {
            start,
            segments: vec![
                Curve::Line {
                    to: Point::new(hx - radius, -hy),
                },
                arc(hx - radius, -hy + radius, -std::f64::consts::FRAC_PI_2, 0.0),
                Curve::Line {
                    to: Point::new(hx, hy - radius),
                },
                arc(hx - radius, hy - radius, 0.0, std::f64::consts::FRAC_PI_2),
                Curve::Line {
                    to: Point::new(-hx + radius, hy),
                },
                arc(
                    -hx + radius,
                    hy - radius,
                    std::f64::consts::FRAC_PI_2,
                    std::f64::consts::PI,
                ),
                Curve::Line {
                    to: Point::new(-hx, -hy + radius),
                },
                arc(
                    -hx + radius,
                    -hy + radius,
                    std::f64::consts::PI,
                    std::f64::consts::PI + std::f64::consts::FRAC_PI_2,
                ),
            ],
        })
    };
    rotate_outline(local, center, rotation_deg)
}

pub fn rectangle(center: Point, width: f64, height: f64) -> Outline {
    let hx = width / 2.0;
    let hy = height / 2.0;
    let start = Point::new(center.x - hx, center.y - hy);
    Outline::Path(PathOutline {
        start,
        segments: vec![
            Curve::Line {
                to: Point::new(center.x + hx, center.y - hy),
            },
            Curve::Line {
                to: Point::new(center.x + hx, center.y + hy),
            },
            Curve::Line {
                to: Point::new(center.x - hx, center.y + hy),
            },
            Curve::Line { to: start },
        ],
    })
}

pub fn polygon(points: &[Point]) -> Outline {
    let mut unique = points.to_vec();
    if unique.len() >= 2 && distance(unique[0], *unique.last().unwrap()) < 1e-7 {
        unique.pop();
    }
    let start = unique.first().copied().unwrap_or(Point::new(0.0, 0.0));
    let mut segments: Vec<Curve> = unique
        .iter()
        .skip(1)
        .map(|point| Curve::Line { to: *point })
        .collect();
    if !unique.is_empty() {
        segments.push(Curve::Line { to: start });
    }
    Outline::Path(PathOutline { start, segments })
}

fn rotate_outline(outline: Outline, origin: Point, rotation_deg: f64) -> Outline {
    match outline {
        Outline::Circle { center, radius } => Outline::Circle {
            center: rotate_point(center, origin, rotation_deg),
            radius,
        },
        Outline::Path(path) => Outline::Path(PathOutline {
            start: rotate_point(path.start, origin, rotation_deg),
            segments: path
                .segments
                .into_iter()
                .map(|curve| match curve {
                    Curve::Line { to } => Curve::Line {
                        to: rotate_point(to, origin, rotation_deg),
                    },
                    Curve::Arc { to, mid } => Curve::Arc {
                        to: rotate_point(to, origin, rotation_deg),
                        mid: rotate_point(mid, origin, rotation_deg),
                    },
                })
                .collect(),
        }),
    }
}

pub fn rotate_point(point: Point, origin: Point, rotation_deg: f64) -> Point {
    if rotation_deg.abs() < EPS && origin.x.abs() < EPS && origin.y.abs() < EPS {
        return point;
    }
    let radians = rotation_deg.to_radians();
    let (sin, cos) = radians.sin_cos();
    Point::new(
        origin.x + point.x * cos - point.y * sin,
        origin.y + point.x * sin + point.y * cos,
    )
}

#[derive(Clone, Copy)]
struct Bounds {
    min_x: f64,
    min_y: f64,
    max_x: f64,
    max_y: f64,
}

fn outline_bounds(outline: &Outline) -> Bounds {
    match outline {
        Outline::Circle { center, radius } => Bounds {
            min_x: center.x - radius,
            min_y: center.y - radius,
            max_x: center.x + radius,
            max_y: center.y + radius,
        },
        Outline::Path(path) => {
            let mut bounds = Bounds {
                min_x: path.start.x,
                min_y: path.start.y,
                max_x: path.start.x,
                max_y: path.start.y,
            };
            let mut cursor = path.start;
            for curve in &path.segments {
                match curve {
                    Curve::Line { to } => {
                        bounds.include(*to);
                        cursor = *to;
                    }
                    Curve::Arc { to, mid } => {
                        bounds.include(*mid);
                        bounds.include(*to);
                        if let Some((center, radius)) = circle_through(cursor, *mid, *to) {
                            for extreme in [
                                Point::new(center.x + radius, center.y),
                                Point::new(center.x - radius, center.y),
                                Point::new(center.x, center.y + radius),
                                Point::new(center.x, center.y - radius),
                            ] {
                                if on_arc(cursor, *mid, *to, center, extreme) {
                                    bounds.include(extreme);
                                }
                            }
                        }
                        cursor = *to;
                    }
                }
            }
            bounds
        }
    }
}

impl Bounds {
    fn include(&mut self, point: Point) {
        self.min_x = self.min_x.min(point.x);
        self.min_y = self.min_y.min(point.y);
        self.max_x = self.max_x.max(point.x);
        self.max_y = self.max_y.max(point.y);
    }
}

fn bounds_meet(bounds: Bounds, window: Window) -> bool {
    bounds.max_x >= window.x_min - EPS
        && bounds.min_x <= window.x_max + EPS
        && bounds.max_y >= window.y_min - EPS
        && bounds.min_y <= window.y_max + EPS
}

fn sample_path(path: &PathOutline, tolerance: f64) -> Vec<Point> {
    let mut points = vec![path.start];
    let mut cursor = path.start;
    for curve in &path.segments {
        match curve {
            Curve::Line { to } => points.push(*to),
            Curve::Arc { to, mid } => {
                if let Some((center, radius)) = circle_through(cursor, *mid, *to) {
                    let sweep = signed_sweep(cursor, *mid, *to, center);
                    let step = chord_step(radius, tolerance);
                    let count = (sweep.abs() / step).ceil().max(1.0) as usize;
                    for index in 1..count {
                        let angle =
                            angle_of(sub(cursor, center)) + sweep * (index as f64 / count as f64);
                        points.push(add(
                            center,
                            Point::new(radius * angle.cos(), radius * angle.sin()),
                        ));
                    }
                }
                points.push(*to);
            }
        }
        cursor = match curve {
            Curve::Line { to } | Curve::Arc { to, .. } => *to,
        };
    }
    if points.len() >= 2 && distance(points[0], *points.last().unwrap()) < 1e-7 {
        points.pop();
    }
    points
}

fn sample_circle(center: Point, radius: f64, tolerance: f64) -> Vec<Point> {
    let step = chord_step(radius, tolerance);
    let count = ((std::f64::consts::TAU) / step).ceil().max(4.0) as usize;
    (0..count)
        .map(|index| {
            let angle = std::f64::consts::TAU * (index as f64 / count as f64);
            add(
                center,
                Point::new(radius * angle.cos(), radius * angle.sin()),
            )
        })
        .collect()
}

fn chord_step(radius: f64, tolerance: f64) -> f64 {
    if radius <= tolerance || tolerance <= 0.0 {
        return std::f64::consts::PI;
    }
    let ratio = (1.0 - tolerance / radius).clamp(-1.0, 1.0);
    2.0 * ratio.acos()
}

fn format_coord(value: f64, step: f64) -> String {
    let snapped = (value / step).round() * step;
    let decimals = step.abs().log10().abs().ceil() as usize;
    let decimals = decimals.min(8);
    let mut text = format!("{snapped:.decimals$}");
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

fn parse_range(text: &str) -> Result<(f64, f64), ()> {
    let (start, rest) = take_number(text)?;
    let rest = rest.strip_prefix('-').ok_or(())?;
    let (end, rest) = take_number(rest)?;
    if rest.is_empty() {
        Ok((start, end))
    } else {
        Err(())
    }
}

fn take_number(text: &str) -> Result<(f64, &str), ()> {
    let bytes = text.as_bytes();
    if bytes.is_empty() {
        return Err(());
    }
    let mut end = 0;
    if bytes[0] == b'+' || bytes[0] == b'-' {
        end = 1;
    }
    let digits_at = end;
    while end < bytes.len() && (bytes[end].is_ascii_digit() || bytes[end] == b'.') {
        end += 1;
    }
    if end == digits_at {
        return Err(());
    }
    let number: f64 = text[..end].parse().map_err(|_| ())?;
    Ok((number, &text[end..]))
}

fn glob_match(pattern: &str, text: &str) -> bool {
    glob_at(pattern.as_bytes(), text.as_bytes())
}

fn glob_at(pattern: &[u8], text: &[u8]) -> bool {
    let mut index = 0;
    let mut text_at = 0;
    while index < pattern.len() {
        if pattern[index] == b'*' {
            if index + 1 == pattern.len() {
                return true;
            }
            for skip in text_at..=text.len() {
                if glob_at(&pattern[index + 1..], &text[skip..]) {
                    return true;
                }
            }
            return false;
        }
        if text_at >= text.len() || pattern[index] != text[text_at] {
            return false;
        }
        index += 1;
        text_at += 1;
    }
    text_at == text.len()
}

fn point_in_path(path: &PathOutline, point: Point) -> bool {
    if distance_to_path(path, point) <= 1e-6 {
        return true;
    }
    let mut crossings = 0;
    let mut cursor = path.start;
    for curve in &path.segments {
        match curve {
            Curve::Line { to } => {
                if segment_crosses_ray(cursor, *to, point) {
                    crossings += 1;
                }
                cursor = *to;
            }
            Curve::Arc { to, mid } => {
                if let Some((center, radius)) = circle_through(cursor, *mid, *to) {
                    for hit in ray_circle(point, Point::new(1.0, 0.0), center, radius) {
                        if hit > 1e-8
                            && on_arc(cursor, *mid, *to, center, add(point, Point::new(hit, 0.0)))
                        {
                            crossings += 1;
                        }
                    }
                }
                cursor = *to;
            }
        }
    }
    crossings % 2 == 1
}

fn distance_to_path(path: &PathOutline, point: Point) -> f64 {
    let mut best = f64::MAX;
    let mut cursor = path.start;
    for curve in &path.segments {
        let next = match curve {
            Curve::Line { to } => {
                best = best.min(distance_to_segment(point, cursor, *to));
                *to
            }
            Curve::Arc { to, mid } => {
                if let Some((center, radius)) = circle_through(cursor, *mid, *to) {
                    let toward = norm(sub(point, center));
                    let candidate = add(center, scale(toward, radius));
                    if on_arc(cursor, *mid, *to, center, candidate) {
                        best = best.min((distance(point, center) - radius).abs());
                    }
                    best = best.min(distance(point, cursor));
                    best = best.min(distance(point, *to));
                }
                *to
            }
        };
        cursor = next;
    }
    best
}

fn segment_crosses_ray(start: Point, end: Point, origin: Point) -> bool {
    let (y0, y1) = (start.y, end.y);
    if (y0 > origin.y) == (y1 > origin.y) {
        return false;
    }
    let x = start.x + (origin.y - start.y) * (end.x - start.x) / (end.y - start.y);
    x > origin.x
}

fn distance_to_segment(point: Point, start: Point, end: Point) -> f64 {
    let length_sq = distance(start, end).powi(2);
    if length_sq < EPS {
        return distance(point, start);
    }
    let t = ((point.x - start.x) * (end.x - start.x) + (point.y - start.y) * (end.y - start.y))
        / length_sq;
    let t = t.clamp(0.0, 1.0);
    distance(point, add(start, scale(sub(end, start), t)))
}

fn ray_segment(origin: Point, direction: Point, start: Point, end: Point) -> Option<f64> {
    let edge = sub(end, start);
    let den = cross(direction, edge);
    if den.abs() < 1e-12 {
        return None;
    }
    let delta = sub(start, origin);
    let t = cross(delta, edge) / den;
    let u = cross(delta, direction) / den;
    if t > 1e-8 && (0.0..=1.0).contains(&u) {
        Some(t)
    } else {
        None
    }
}

fn ray_circle(origin: Point, direction: Point, center: Point, radius: f64) -> Vec<f64> {
    let offset = sub(origin, center);
    let a = dot(direction, direction);
    let b = 2.0 * dot(offset, direction);
    let c = dot(offset, offset) - radius * radius;
    let disc = b * b - 4.0 * a * c;
    if disc < 0.0 || a.abs() < EPS {
        return Vec::new();
    }
    let root = disc.sqrt();
    let mut hits = Vec::new();
    for sign in [-1.0, 1.0] {
        let t = (-b + sign * root) / (2.0 * a);
        if t > 1e-8 {
            hits.push(t);
        }
    }
    hits.sort_by(f64::total_cmp);
    hits
}

fn on_arc(start: Point, mid: Point, end: Point, center: Point, point: Point) -> bool {
    let sweep = signed_sweep(start, mid, end, center);
    let at = signed_delta(
        angle_of(sub(point, center)) - angle_of(sub(start, center)),
        sweep,
    );
    (at.abs() <= sweep.abs() + 1e-4) && at.signum() == sweep.signum() || at.abs() < 1e-4
}

fn signed_sweep(start: Point, mid: Point, end: Point, center: Point) -> f64 {
    let a0 = angle_of(sub(start, center));
    let am = angle_of(sub(mid, center));
    let a1 = angle_of(sub(end, center));
    let mut to_mid = am - a0;
    let mut to_end = a1 - a0;
    if to_mid > std::f64::consts::PI {
        to_mid -= std::f64::consts::TAU;
    }
    if to_mid < -std::f64::consts::PI {
        to_mid += std::f64::consts::TAU;
    }
    if to_end > std::f64::consts::PI {
        to_end -= std::f64::consts::TAU;
    }
    if to_end < -std::f64::consts::PI {
        to_end += std::f64::consts::TAU;
    }
    if to_mid.signum() == to_end.signum() && to_end.abs() + 1e-6 >= to_mid.abs() {
        to_end
    } else if to_mid > 0.0 {
        if to_end > 0.0 {
            to_end
        } else {
            to_end + std::f64::consts::TAU
        }
    } else if to_end < 0.0 {
        to_end
    } else {
        to_end - std::f64::consts::TAU
    }
}

fn signed_delta(mut delta: f64, sweep: f64) -> f64 {
    if sweep >= 0.0 {
        while delta < -1e-6 {
            delta += std::f64::consts::TAU;
        }
        while delta > std::f64::consts::TAU {
            delta -= std::f64::consts::TAU;
        }
    } else {
        while delta > 1e-6 {
            delta -= std::f64::consts::TAU;
        }
        while delta < -std::f64::consts::TAU {
            delta += std::f64::consts::TAU;
        }
    }
    delta
}

fn circle_through(a: Point, b: Point, c: Point) -> Option<(Point, f64)> {
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
    Some((center, distance(center, a)))
}

fn offset_path(path: &PathOutline, distance: f64) -> PathOutline {
    let edges = path_edges(path);
    if edges.is_empty() {
        return path.clone();
    }
    if edges.iter().all(|edge| matches!(edge, Edge::Line { .. })) {
        let mut points: Vec<Point> = Vec::new();
        if let Edge::Line { a, .. } = &edges[0] {
            points.push(*a);
        }
        for edge in &edges {
            if let Edge::Line { b, .. } = edge {
                if distance_pts(*points.last().unwrap(), *b) > 1e-7 {
                    points.push(*b);
                }
            }
        }
        if points.len() >= 2 && distance_pts(points[0], *points.last().unwrap()) < 1e-7 {
            points.pop();
        }
        if points.len() >= 3 {
            return offset_polygon(&points, distance);
        }
    }
    offset_curved(&edges, distance).unwrap_or_else(|| path.clone())
}

enum Edge {
    Line { a: Point, b: Point },
    Arc { a: Point, b: Point, mid: Point },
}

fn path_edges(path: &PathOutline) -> Vec<Edge> {
    let mut cursor = path.start;
    let mut edges = Vec::new();
    for curve in &path.segments {
        match curve {
            Curve::Line { to } => {
                if distance(cursor, *to) > 1e-9 {
                    edges.push(Edge::Line { a: cursor, b: *to });
                }
                cursor = *to;
            }
            Curve::Arc { to, mid } => {
                edges.push(Edge::Arc {
                    a: cursor,
                    b: *to,
                    mid: *mid,
                });
                cursor = *to;
            }
        }
    }
    edges
}

struct Corner {
    arrive: Point,
    leave: Point,
    mid: Option<Point>,
}

fn offset_polygon(points: &[Point], distance: f64) -> PathOutline {
    let points = if signed_area(points) < 0.0 {
        let mut reversed = points.to_vec();
        reversed.reverse();
        reversed
    } else {
        points.to_vec()
    };
    let count = points.len();
    let mut corners = Vec::with_capacity(count);
    for index in 0..count {
        let prev = points[(index + count - 1) % count];
        let current = points[index];
        let next = points[(index + 1) % count];
        let inward_dir = norm(sub(current, prev));
        let outward_dir = norm(sub(next, current));
        let n_in = right_normal(inward_dir);
        let n_out = right_normal(outward_dir);
        let arrive = add(current, scale(n_in, distance));
        let leave = add(current, scale(n_out, distance));
        let turn = cross(inward_dir, outward_dir);
        let convex = turn > 1e-8;
        let use_arc = if distance >= 0.0 { convex } else { !convex };
        if use_arc && distance_pts(arrive, leave) > 1e-7 {
            let bisector = norm(add(n_in, n_out));
            let mid = add(current, scale(bisector, distance.abs()));
            corners.push(Corner {
                arrive,
                leave,
                mid: Some(mid),
            });
        } else {
            let hit = line_intersect(
                add(prev, scale(n_in, distance)),
                inward_dir,
                add(current, scale(n_out, distance)),
                outward_dir,
            )
            .unwrap_or(arrive);
            corners.push(Corner {
                arrive: hit,
                leave: hit,
                mid: None,
            });
        }
    }
    corners_to_path(&corners)
}

fn corners_to_path(corners: &[Corner]) -> PathOutline {
    let start = corners[0].arrive;
    let mut segments = Vec::new();
    let mut cursor = start;
    for (index, corner) in corners.iter().enumerate() {
        if index > 0 && distance_pts(cursor, corner.arrive) > 1e-7 {
            segments.push(Curve::Line { to: corner.arrive });
            cursor = corner.arrive;
        }
        if let Some(mid) = corner.mid {
            if distance_pts(cursor, corner.leave) > 1e-7 {
                segments.push(Curve::Arc {
                    to: corner.leave,
                    mid,
                });
                cursor = corner.leave;
            }
        }
    }
    if distance_pts(cursor, start) > 1e-7 {
        segments.push(Curve::Line { to: start });
    }
    PathOutline { start, segments }
}

fn offset_curved(edges: &[Edge], distance: f64) -> Option<PathOutline> {
    let mut offset = Vec::new();
    for edge in edges {
        offset.push(offset_edge(edge, distance)?);
    }
    let start = offset[0].0;
    let mut segments = Vec::new();
    let mut cursor = start;
    for (index, (a, b, mid)) in offset.iter().enumerate() {
        if distance_pts(cursor, *a) > 1e-6 {
            let vertex = match &edges[index] {
                Edge::Line { a, .. } | Edge::Arc { a, .. } => *a,
            };
            let mid_point = add(
                vertex,
                scale(
                    norm(add(sub(cursor, vertex), sub(*a, vertex))),
                    distance.abs(),
                ),
            );
            segments.push(Curve::Arc {
                to: *a,
                mid: mid_point,
            });
        }
        match mid {
            Some(mid) => segments.push(Curve::Arc { to: *b, mid: *mid }),
            None => segments.push(Curve::Line { to: *b }),
        }
        cursor = *b;
    }
    if distance_pts(cursor, start) > 1e-6 {
        segments.push(Curve::Line { to: start });
    }
    Some(PathOutline { start, segments })
}

fn offset_edge(edge: &Edge, distance: f64) -> Option<(Point, Point, Option<Point>)> {
    match edge {
        Edge::Line { a, b } => {
            let normal = right_normal(norm(sub(*b, *a)));
            Some((
                add(*a, scale(normal, distance)),
                add(*b, scale(normal, distance)),
                None,
            ))
        }
        Edge::Arc { a, b, mid } => {
            let (center, radius) = circle_through(*a, *mid, *b)?;
            let motion = sub(*mid, *a);
            let ccw = cross(motion, sub(center, *a)) > 0.0;
            let grow = if ccw { distance } else { -distance };
            let grown = (radius + grow).max(0.0);
            let place = |point: Point| add(center, scale(norm(sub(point, center)), grown));
            Some((place(*a), place(*b), Some(place(*mid))))
        }
    }
}

fn signed_area(points: &[Point]) -> f64 {
    let mut area = 0.0;
    for index in 0..points.len() {
        let next = points[(index + 1) % points.len()];
        area += points[index].x * next.y - next.x * points[index].y;
    }
    area / 2.0
}

fn line_intersect(
    origin: Point,
    direction: Point,
    other: Point,
    other_dir: Point,
) -> Option<Point> {
    let den = cross(direction, other_dir);
    if den.abs() < 1e-12 {
        return None;
    }
    let t = cross(sub(other, origin), other_dir) / den;
    Some(add(origin, scale(direction, t)))
}

fn right_normal(direction: Point) -> Point {
    Point::new(direction.y, -direction.x)
}

fn angle_of(point: Point) -> f64 {
    point.y.atan2(point.x)
}

fn distance(a: Point, b: Point) -> f64 {
    let d = sub(a, b);
    (d.x * d.x + d.y * d.y).sqrt()
}

fn distance_pts(a: Point, b: Point) -> f64 {
    distance(a, b)
}

fn sub(a: Point, b: Point) -> Point {
    Point::new(a.x - b.x, a.y - b.y)
}

fn add(a: Point, b: Point) -> Point {
    Point::new(a.x + b.x, a.y + b.y)
}

fn scale(point: Point, factor: f64) -> Point {
    Point::new(point.x * factor, point.y * factor)
}

fn dot(a: Point, b: Point) -> f64 {
    a.x * b.x + a.y * b.y
}

fn cross(a: Point, b: Point) -> f64 {
    a.x * b.y - a.y * b.x
}

fn norm(point: Point) -> Point {
    let length = (point.x * point.x + point.y * point.y).sqrt();
    if length < EPS {
        Point::new(0.0, 0.0)
    } else {
        scale(point, 1.0 / length)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules() -> ClearanceRules {
        ClearanceRules {
            min_clearance: 0.15,
            min_hole_clearance: 0.25,
            min_copper_edge_clearance: 0.5,
            classes: vec![
                NetClass {
                    name: "Default".into(),
                    clearance: 0.2,
                    priority: 100,
                },
                NetClass {
                    name: "Power".into(),
                    clearance: 0.3,
                    priority: 2,
                },
            ],
            patterns: vec![NetPattern {
                class_name: "Power".into(),
                pattern: "/VB*".into(),
            }],
        }
    }

    #[test]
    fn window_parses_a_negative_span() {
        let window = parse_window("-10--5x0-40").unwrap();
        assert_eq!(window.x_min, -10.0);
        assert_eq!(window.x_max, -5.0);
        assert_eq!(window.y_max, 40.0);
    }

    #[test]
    fn net_class_pattern_beats_the_default() {
        let rules = rules();
        assert!((rules.for_net(Some("/VBUS")) - 0.3).abs() < 1e-9);
        assert!((rules.for_net(Some("GND")) - 0.2).abs() < 1e-9);
        assert!((rules.for_net(None) - 0.2).abs() < 1e-9);
    }

    #[test]
    fn a_square_grows_by_the_clearance_including_the_corner_arc() {
        let outline = polygon(&[
            Point::new(0.0, 0.0),
            Point::new(2.0, 0.0),
            Point::new(2.0, 1.0),
            Point::new(0.0, 1.0),
        ]);
        let grown = offset_outline(&outline, 0.5);
        assert!(contains(&grown, Point::new(-0.4, 0.5)));
        assert!(!contains(&grown, Point::new(-0.6, 0.5)));
        assert!(contains(&grown, Point::new(-0.2, -0.2)));
        assert!(contains(&outline, Point::new(1.0, 0.5)));
    }

    #[test]
    fn track_clearance_is_the_stadium_plus_the_gap() {
        let mut board = BoardGeometry {
            copper_layers: vec!["F.Cu".into()],
            shapes: vec![Shape {
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
                inset: false,
                inward: false,
                occupied: stadium(Point::new(0.0, 0.0), Point::new(10.0, 0.0), 0.25),
                clearance: stadium(Point::new(0.0, 0.0), Point::new(10.0, 0.0), 0.25),
            }],
        };
        apply_clearance(&mut board, &rules());
        let shape = &board.shapes[0];
        assert!((shape.clearance_mm - 0.2).abs() < 1e-9);
        let beside = Point::new(5.0, 0.2);
        assert!(!contains(&shape.occupied, beside));
        assert!(contains(&shape.clearance, beside));
        assert!(!contains(&shape.clearance, Point::new(5.0, 0.4)));
    }

    #[test]
    fn clearance_bounds_pull_a_shape_into_the_window() {
        let mut board = BoardGeometry {
            copper_layers: vec!["F.Cu".into()],
            shapes: vec![Shape {
                source: Source::Zone,
                layer: "F.Cu".into(),
                reference: None,
                pad: None,
                net: Some("GND".into()),
                width: None,
                blocks: true,
                clearance_class: ClearanceClass::Copper {
                    net: Some("GND".into()),
                },
                clearance_mm: 0.0,
                inset: false,
                inward: false,
                occupied: polygon(&[
                    Point::new(0.0, 0.0),
                    Point::new(4.0, 0.0),
                    Point::new(4.0, 2.0),
                    Point::new(0.0, 2.0),
                ]),
                clearance: polygon(&[
                    Point::new(0.0, 0.0),
                    Point::new(4.0, 0.0),
                    Point::new(4.0, 2.0),
                    Point::new(0.0, 2.0),
                ]),
            }],
        };
        apply_clearance(&mut board, &rules());
        let window = parse_window("1-2x2.05-3").unwrap();
        assert!(shape_meets_window(&board.shapes[0], window));
        let missed = parse_window("1-2x3-4").unwrap();
        assert!(!shape_meets_window(&board.shapes[0], missed));
    }

    #[test]
    fn detailed_points_round_and_an_arc_stays_within_the_step() {
        let center = Point::new(0.0, 0.0);
        let outline = Outline::Circle {
            center,
            radius: 1.0,
        };
        let points = sample_outline(&outline, 0.01);
        let formatted = format_points(&points, 0.01);
        assert!(formatted.iter().all(|text| text.contains('x')));
        for point in &points {
            let error = (distance(*point, center) - 1.0).abs();
            assert!(error < 1e-6);
        }
        assert!(points.len() > 8);
        let snapped = format_points(&[Point::new(0.1, 5.5), Point::new(0.104, 5.5)], 0.01);
        assert_eq!(snapped, vec!["0.1x5.5".to_string()]);
    }
}
