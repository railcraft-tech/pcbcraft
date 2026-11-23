//! SVG elliptical arcs, as EasyEDA stores them in an `ARC` shape.

const PI: f64 = std::f64::consts::PI;

pub fn midpoint(path: &str) -> Option<((f64, f64), (f64, f64), (f64, f64))> {
    let arc = parse(path)?;
    let (center_x, center_y, theta, extent, radius_x, radius_y, phi) = geometry(&arc)?;
    let start = arc.start;
    let end = arc.end;
    let mid = point_at(
        center_x,
        center_y,
        radius_x,
        radius_y,
        phi,
        theta + extent / 2.0,
    );
    Some((start, mid, end))
}

struct SvgArc {
    start: (f64, f64),
    end: (f64, f64),
    radius_x: f64,
    radius_y: f64,
    phi: f64,
    large: bool,
    sweep: bool,
}

fn parse(path: &str) -> Option<SvgArc> {
    let text = path.replace(',', " ");
    let tokens: Vec<&str> = text.split_whitespace().collect();
    let move_at = tokens
        .iter()
        .position(|token| token.eq_ignore_ascii_case("M"))?;
    let arc_at = tokens
        .iter()
        .position(|token| token.eq_ignore_ascii_case("A"))?;
    if arc_at + 7 >= tokens.len() || move_at + 2 >= tokens.len() {
        return None;
    }
    Some(SvgArc {
        start: (number(tokens[move_at + 1])?, number(tokens[move_at + 2])?),
        radius_x: number(tokens[arc_at + 1])?,
        radius_y: number(tokens[arc_at + 2])?,
        phi: number(tokens[arc_at + 3])?,
        large: tokens[arc_at + 4] == "1",
        sweep: tokens[arc_at + 5] == "1",
        end: (number(tokens[arc_at + 6])?, number(tokens[arc_at + 7])?),
    })
}

fn number(text: &str) -> Option<f64> {
    let value: f64 = text.parse().ok()?;
    if value.is_finite() {
        Some(value)
    } else {
        None
    }
}

/// Center parameterization from the SVG implementation notes.
fn geometry(arc: &SvgArc) -> Option<(f64, f64, f64, f64, f64, f64, f64)> {
    let phi = arc.phi.to_radians();
    let (cos_phi, sin_phi) = (phi.cos(), phi.sin());
    let dx = (arc.start.0 - arc.end.0) / 2.0;
    let dy = (arc.start.1 - arc.end.1) / 2.0;
    let x1 = cos_phi * dx + sin_phi * dy;
    let y1 = -sin_phi * dx + cos_phi * dy;

    let mut radius_x = arc.radius_x.abs();
    let mut radius_y = arc.radius_y.abs();
    if radius_x == 0.0 || radius_y == 0.0 {
        return None;
    }
    let lambda = x1 * x1 / (radius_x * radius_x) + y1 * y1 / (radius_y * radius_y);
    if lambda > 1.0 {
        let scale = lambda.sqrt();
        radius_x *= scale;
        radius_y *= scale;
    }

    let numerator = (radius_x * radius_x) * (radius_y * radius_y)
        - (radius_x * radius_x) * (y1 * y1)
        - (radius_y * radius_y) * (x1 * x1);
    let denominator = (radius_x * radius_x) * (y1 * y1) + (radius_y * radius_y) * (x1 * x1);
    if denominator == 0.0 {
        return None;
    }
    let sign = if arc.large == arc.sweep { -1.0 } else { 1.0 };
    let coef = sign * (numerator / denominator).max(0.0).sqrt();
    let center_x1 = coef * ((radius_x * y1) / radius_y);
    let center_y1 = coef * -((radius_y * x1) / radius_x);

    let mid_x = (arc.start.0 + arc.end.0) / 2.0;
    let mid_y = (arc.start.1 + arc.end.1) / 2.0;
    let center_x = mid_x + cos_phi * center_x1 - sin_phi * center_y1;
    let center_y = mid_y + sin_phi * center_x1 + cos_phi * center_y1;

    let theta = vector_angle(
        1.0,
        0.0,
        (x1 - center_x1) / radius_x,
        (y1 - center_y1) / radius_y,
    );
    let mut extent = vector_angle(
        (x1 - center_x1) / radius_x,
        (y1 - center_y1) / radius_y,
        (-x1 - center_x1) / radius_x,
        (-y1 - center_y1) / radius_y,
    );
    if !arc.sweep && extent > 0.0 {
        extent -= 360.0;
    } else if arc.sweep && extent < 0.0 {
        extent += 360.0;
    }
    Some((center_x, center_y, theta, extent, radius_x, radius_y, phi))
}

fn vector_angle(ux: f64, uy: f64, vx: f64, vy: f64) -> f64 {
    let sign = if ux * vy - uy * vx < 0.0 { -1.0 } else { 1.0 };
    let dot = ux * vx + uy * vy;
    let length = ((ux * ux + uy * uy) * (vx * vx + vy * vy)).sqrt();
    if length == 0.0 {
        return 0.0;
    }
    sign * (dot / length).clamp(-1.0, 1.0).acos() * 180.0 / PI
}

fn point_at(cx: f64, cy: f64, rx: f64, ry: f64, phi: f64, theta_deg: f64) -> (f64, f64) {
    let theta = theta_deg.to_radians();
    let local_x = rx * theta.cos();
    let local_y = ry * theta.sin();
    (
        cx + phi.cos() * local_x - phi.sin() * local_y,
        cy + phi.sin() * local_x + phi.cos() * local_y,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semicircle_midpoint_is_the_bulge() {
        let (start, mid, end) = midpoint("M 0 0 A 5 5 0 0 1 10 0").unwrap();
        assert_eq!(start, (0.0, 0.0));
        assert_eq!(end, (10.0, 0.0));
        assert!((mid.0 - 5.0).abs() < 1e-6, "{mid:?}");
        assert!((mid.1 + 5.0).abs() < 1e-6, "{mid:?}");
    }
}
