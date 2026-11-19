//! Distance between filled outlines. Arcs are measured on their circle, not as chords.

use super::{
    add, angle_of, circle_through, contains, distance, distance_to_path, distance_to_segment,
    on_arc, scale, sub, Curve, Outline, PathOutline, Point,
};

const TOUCH: f64 = 1e-4;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Separation {
    pub distance: f64,
    pub at: Point,
}

pub fn filled_separation(a: &Outline, b: &Outline) -> Separation {
    if contains_other(a, b) || contains_other(b, a) {
        return Separation {
            distance: 0.0,
            at: anchor(a),
        };
    }
    boundary_gap(a, b)
}

/// Straight edges stay exact. Arcs become chords within 0.01 mm of the curve.
pub fn boundary_segments(outline: &Outline) -> Vec<(Point, Point)> {
    match outline {
        Outline::Circle { center, radius } => {
            let count = 24;
            (0..count)
                .map(|index| {
                    let a0 = std::f64::consts::TAU * (index as f64 / count as f64);
                    let a1 = std::f64::consts::TAU * ((index + 1) as f64 / count as f64);
                    (
                        Point::new(center.x + radius * a0.cos(), center.y + radius * a0.sin()),
                        Point::new(center.x + radius * a1.cos(), center.y + radius * a1.sin()),
                    )
                })
                .collect()
        }
        Outline::Path(path) => {
            let mut out = Vec::new();
            let mut cursor = path.start;
            for curve in &path.segments {
                match curve {
                    Curve::Line { to } => out.push((cursor, *to)),
                    Curve::Arc { to, mid } => {
                        if let Some((center, radius)) = circle_through(cursor, *mid, *to) {
                            let sweep = signed_sweep_public(cursor, *mid, *to, center);
                            let step = 0.01_f64.max(radius.abs() * 0.05);
                            let count = (sweep.abs() * radius / step).ceil().max(1.0) as usize;
                            let mut previous = cursor;
                            for index in 1..count {
                                let angle = angle_of(sub(cursor, center))
                                    + sweep * (index as f64 / count as f64);
                                let point = Point::new(
                                    center.x + radius * angle.cos(),
                                    center.y + radius * angle.sin(),
                                );
                                out.push((previous, point));
                                previous = point;
                            }
                            out.push((previous, *to));
                        } else {
                            out.push((cursor, *to));
                        }
                    }
                }
                cursor = match curve {
                    Curve::Line { to } | Curve::Arc { to, .. } => *to,
                };
            }
            out
        }
    }
}

fn signed_sweep_public(start: Point, mid: Point, end: Point, center: Point) -> f64 {
    super::signed_sweep(start, mid, end, center)
}

pub fn boundary_gap(a: &Outline, b: &Outline) -> Separation {
    match (a, b) {
        (
            Outline::Circle {
                center: ca,
                radius: ra,
            },
            Outline::Circle {
                center: cb,
                radius: rb,
            },
        ) => circle_circle(*ca, *ra, *cb, *rb),
        (Outline::Circle { center, radius }, Outline::Path(path))
        | (Outline::Path(path), Outline::Circle { center, radius }) => {
            circle_path(*center, *radius, path)
        }
        (Outline::Path(left), Outline::Path(right)) => path_path(left, right),
    }
}

fn contains_other(outer: &Outline, inner: &Outline) -> bool {
    match (outer, inner) {
        (Outline::Circle { center, radius }, other) => {
            boundary_gap(outer, other).distance > TOUCH
                && distance(*center, anchor(other)) < *radius
        }
        (Outline::Path(path), Outline::Circle { center, radius }) => {
            contains(outer, *center) && distance_to_path(path, *center) > *radius + TOUCH
        }
        (Outline::Path(_), Outline::Path(path)) => {
            contains(outer, path.start) && boundary_gap(outer, inner).distance > TOUCH
        }
    }
}

fn circle_circle(ca: Point, ra: f64, cb: Point, rb: f64) -> Separation {
    let apart = distance(ca, cb);
    let gap = if apart >= ra + rb {
        apart - ra - rb
    } else if apart <= (ra - rb).abs() {
        (ra - rb).abs() - apart
    } else {
        0.0
    };
    let at = if apart < TOUCH {
        ca
    } else {
        let toward = scale(sub(cb, ca), 0.5);
        add(ca, toward)
    };
    Separation { distance: gap, at }
}

fn circle_path(center: Point, radius: f64, path: &PathOutline) -> Separation {
    let mut best = f64::MAX;
    let mut at = center;
    let mut cursor = path.start;
    for curve in &path.segments {
        let (gap, hit, next) = match curve {
            Curve::Line { to } => {
                let (gap, hit) = point_segment(center, cursor, *to, radius);
                (gap, hit, *to)
            }
            Curve::Arc { to, mid } => {
                let (gap, hit) = point_arc(center, cursor, *mid, *to, radius);
                (gap, hit, *to)
            }
        };
        if gap < best {
            best = gap;
            at = hit;
        }
        cursor = next;
    }
    Separation { distance: best, at }
}

fn point_segment(center: Point, start: Point, end: Point, radius: f64) -> (f64, Point) {
    let dist = distance_to_segment(center, start, end);
    (
        (dist - radius).max(0.0),
        closest_on_segment(center, start, end),
    )
}

fn point_arc(center: Point, start: Point, mid: Point, end: Point, radius: f64) -> (f64, Point) {
    let Some((arc_center, arc_radius)) = circle_through(start, mid, end) else {
        return point_segment(center, start, end, radius);
    };
    let mut best = f64::MAX;
    let mut at = start;
    for candidate in [start, end] {
        let gap = (distance(center, candidate) - radius).max(0.0);
        if gap < best {
            best = gap;
            at = candidate;
        }
    }
    let toward = sub(center, arc_center);
    let length = distance(toward, Point::new(0.0, 0.0));
    if length > TOUCH {
        let on = add(arc_center, scale(toward, arc_radius / length));
        if on_arc(start, mid, end, arc_center, on) {
            let gap = (distance(center, on) - radius).max(0.0);
            if gap < best {
                best = gap;
                at = on;
            }
        }
    }
    (best, at)
}

fn path_path(left: &PathOutline, right: &PathOutline) -> Separation {
    let mut best = f64::MAX;
    let mut at = left.start;
    let left_curves = curves(left);
    let right_curves = curves(right);
    for (a0, a_curve) in &left_curves {
        for (b0, b_curve) in &right_curves {
            let (gap, hit) = curve_curve(*a0, a_curve, *b0, b_curve);
            if gap < best {
                best = gap;
                at = hit;
            }
        }
    }
    Separation { distance: best, at }
}

fn curves(path: &PathOutline) -> Vec<(Point, Curve)> {
    let mut cursor = path.start;
    let mut out = Vec::new();
    for curve in &path.segments {
        out.push((cursor, curve.clone()));
        cursor = match curve {
            Curve::Line { to } | Curve::Arc { to, .. } => *to,
        };
    }
    out
}

fn curve_curve(a0: Point, a: &Curve, b0: Point, b: &Curve) -> (f64, Point) {
    match (a, b) {
        (Curve::Line { to: a1 }, Curve::Line { to: b1 }) => segment_segment(a0, *a1, b0, *b1),
        (Curve::Line { to: a1 }, Curve::Arc { to: b1, mid }) => segment_arc(a0, *a1, b0, *mid, *b1),
        (Curve::Arc { to: a1, mid }, Curve::Line { to: b1 }) => {
            let (gap, at) = segment_arc(b0, *b1, a0, *mid, *a1);
            (gap, at)
        }
        (Curve::Arc { to: a1, mid: am }, Curve::Arc { to: b1, mid: bm }) => {
            arc_arc(a0, *am, *a1, b0, *bm, *b1)
        }
    }
}

fn segment_segment(a0: Point, a1: Point, b0: Point, b1: Point) -> (f64, Point) {
    let u = sub(a1, a0);
    let v = sub(b1, b0);
    let w = sub(a0, b0);
    let uu = dot(u, u);
    let vv = dot(v, v);
    let uv = dot(u, v);
    let uw = dot(u, w);
    let vw = dot(v, w);
    let denom = uu * vv - uv * uv;
    let (s, t) = if uu < TOUCH && vv < TOUCH {
        (0.0, 0.0)
    } else if uu < TOUCH {
        (0.0, (vw / vv).clamp(0.0, 1.0))
    } else if vv < TOUCH {
        ((-uw / uu).clamp(0.0, 1.0), 0.0)
    } else if denom.abs() < 1e-12 {
        ((-uw / uu).clamp(0.0, 1.0), 0.0)
    } else {
        let s = ((uv * vw - vv * uw) / denom).clamp(0.0, 1.0);
        let t = ((uv * s + vw) / vv).clamp(0.0, 1.0);
        let s = ((uv * t - uw) / uu).clamp(0.0, 1.0);
        (s, t)
    };
    let pa = add(a0, scale(u, s));
    let pb = add(b0, scale(v, t));
    let gap = distance(pa, pb);
    (gap, Point::new((pa.x + pb.x) / 2.0, (pa.y + pb.y) / 2.0))
}

fn segment_arc(a0: Point, a1: Point, start: Point, mid: Point, end: Point) -> (f64, Point) {
    let Some((center, radius)) = circle_through(start, mid, end) else {
        return segment_segment(a0, a1, start, end);
    };
    let mut best = f64::MAX;
    let mut at = start;
    for (gap, hit) in [segment_point(a0, a1, start), segment_point(a0, a1, end)] {
        if gap < best {
            best = gap;
            at = hit;
        }
    }
    let foot = closest_on_segment(center, a0, a1);
    let away = sub(foot, center);
    let length = distance(away, Point::new(0.0, 0.0));
    if length > TOUCH {
        let on = add(center, scale(away, radius / length));
        if on_arc(start, mid, end, center, on) {
            let gap = distance(foot, on);
            if gap < best {
                best = gap;
                at = Point::new((foot.x + on.x) / 2.0, (foot.y + on.y) / 2.0);
            }
        }
    }
    // The infinite line may miss the arc while an endpoint of the segment is closer
    // to the arc interior. Project the segment endpoints onto the arc as well.
    for end_point in [a0, a1] {
        let (gap, hit) = point_arc(end_point, start, mid, end, 0.0);
        if gap < best {
            best = gap;
            at = hit;
        }
    }
    (best, at)
}

fn arc_arc(a0: Point, am: Point, a1: Point, b0: Point, bm: Point, b1: Point) -> (f64, Point) {
    let mut best = f64::MAX;
    let mut at = a0;
    for (gap, hit) in [
        point_arc(a0, b0, bm, b1, 0.0),
        point_arc(a1, b0, bm, b1, 0.0),
        point_arc(b0, a0, am, a1, 0.0),
        point_arc(b1, a0, am, a1, 0.0),
    ] {
        if gap < best {
            best = gap;
            at = hit;
        }
    }
    if let (Some((ca, ra)), Some((cb, rb))) =
        (circle_through(a0, am, a1), circle_through(b0, bm, b1))
    {
        let apart = distance(ca, cb);
        if apart > TOUCH {
            let dir = scale(sub(cb, ca), 1.0 / apart);
            let on_a = add(ca, scale(dir, ra));
            let on_b = add(cb, scale(dir, -rb));
            if on_arc(a0, am, a1, ca, on_a) && on_arc(b0, bm, b1, cb, on_b) {
                let gap = (apart - ra - rb).max(0.0);
                if gap < best {
                    best = gap;
                    at = Point::new((on_a.x + on_b.x) / 2.0, (on_a.y + on_b.y) / 2.0);
                }
            }
        }
    }
    (best, at)
}

fn segment_point(start: Point, end: Point, point: Point) -> (f64, Point) {
    let hit = closest_on_segment(point, start, end);
    (
        distance(hit, point),
        Point::new((hit.x + point.x) / 2.0, (hit.y + point.y) / 2.0),
    )
}

fn closest_on_segment(point: Point, start: Point, end: Point) -> Point {
    let length_sq = distance(start, end).powi(2);
    if length_sq < TOUCH {
        return start;
    }
    let t = ((point.x - start.x) * (end.x - start.x) + (point.y - start.y) * (end.y - start.y))
        / length_sq;
    add(start, scale(sub(end, start), t.clamp(0.0, 1.0)))
}

fn dot(a: Point, b: Point) -> f64 {
    a.x * b.x + a.y * b.y
}

fn anchor(outline: &Outline) -> Point {
    match outline {
        Outline::Circle { center, .. } => *center,
        Outline::Path(path) => path.start,
    }
}

#[allow(dead_code)]
fn _angle(point: Point) -> f64 {
    angle_of(point)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::{polygon, rectangle, round_rect};

    #[test]
    fn rectangles_report_the_parallel_gap() {
        let left = rectangle(Point::new(0.0, 0.0), 1.0, 1.0);
        let right = rectangle(Point::new(1.4, 0.0), 1.0, 1.0);
        let gap = filled_separation(&left, &right);
        assert!((gap.distance - 0.4).abs() < 1e-6, "{}", gap.distance);
    }

    #[test]
    fn a_circle_inside_a_rectangle_has_no_copper_gap() {
        let board = rectangle(Point::new(0.0, 0.0), 10.0, 10.0);
        let pad = Outline::Circle {
            center: Point::new(0.0, 0.0),
            radius: 0.5,
        };
        assert_eq!(filled_separation(&board, &pad).distance, 0.0);
        let edge = boundary_gap(&board, &pad).distance;
        assert!(edge > 4.0, "{edge}");
    }

    #[test]
    fn round_rect_corners_do_not_shrink_a_flat_gap() {
        let left = round_rect(Point::new(0.0, 0.0), 1.0, 0.4, 0.05, 0.0);
        let right = round_rect(Point::new(1.3, 0.0), 1.0, 0.4, 0.05, 0.0);
        let gap = filled_separation(&left, &right).distance;
        assert!((gap - 0.3).abs() < 1e-3, "{gap}");
    }

    #[test]
    fn an_arc_corner_is_closer_than_the_bounding_box() {
        let left = polygon(&[
            Point::new(0.0, 0.0),
            Point::new(1.0, 0.0),
            Point::new(1.0, 1.0),
            Point::new(0.0, 1.0),
        ]);
        let right = round_rect(Point::new(1.2, 1.2), 0.4, 0.4, 0.1, 0.0);
        let gap = filled_separation(&left, &right).distance;
        let fillet = (2.0_f64.sqrt() - 1.0) * 0.1;
        assert!((gap - fillet).abs() < 1e-6, "{gap}");
    }
}
