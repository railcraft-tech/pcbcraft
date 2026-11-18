use super::value::{band, Tolerance};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Span {
    pub min: f64,
    pub max: f64,
}

impl Span {
    pub fn new(min: f64, max: f64) -> Self {
        Self {
            min: min.min(max),
            max: min.max(max),
        }
    }

    pub fn point(value: f64) -> Self {
        Self {
            min: value,
            max: value,
        }
    }
}

/// Largest voltage difference between two inclusive ranges.
pub fn worst_delta(left: Span, right: Span) -> f64 {
    (left.max - right.min).max(right.max - left.min).max(0.0)
}

/// `Vmax² / Rmin`, with `Rmin` taken from the low end of the tolerance band.
pub fn worst_power(left: Span, right: Span, nominal: f64, tolerance: Tolerance) -> f64 {
    let (low, _) = band(nominal, tolerance);
    let resistance = low.max(1e-15);
    let voltage = worst_delta(left, right);
    voltage * voltage / resistance
}

/// Divider output when `r_a` sits on rail `a` and `r_b` sits on rail `b`.
pub fn divider_span(
    a: Span,
    b: Span,
    r_a: f64,
    tol_a: Tolerance,
    r_b: f64,
    tol_b: Tolerance,
) -> Span {
    let mut span = Span::point(0.0);
    let mut first = true;
    for (ra, rb, va, vb) in corners(r_a, tol_a, r_b, tol_b, a, b) {
        let value = (va * rb + vb * ra) / (ra + rb);
        if first {
            span = Span::point(value);
            first = false;
        } else {
            span.min = span.min.min(value);
            span.max = span.max.max(value);
        }
    }
    span
}

pub fn tau_span(resistance: f64, r_tol: Tolerance, capacitance: f64, c_tol: Tolerance) -> Span {
    let (r_low, r_high) = band(resistance, r_tol);
    let (c_low, c_high) = band(capacitance, c_tol);
    Span::new(r_low * c_low, r_high * c_high)
}

pub fn series_resistance(parts: &[(f64, Tolerance)]) -> Span {
    let mut min = 0.0;
    let mut max = 0.0;
    for (nominal, tolerance) in parts {
        let (low, high) = band(*nominal, *tolerance);
        min += low;
        max += high;
    }
    Span::new(min, max)
}

pub fn parallel_resistance(parts: &[(f64, Tolerance)]) -> Span {
    let mut min = 0.0;
    let mut max = 0.0;
    for (nominal, tolerance) in parts {
        let (low, high) = band(*nominal, *tolerance);
        min += 1.0 / low.max(1e-15);
        max += 1.0 / high.max(1e-15);
    }
    Span::new(1.0 / min, 1.0 / max)
}

pub fn series_capacitance(parts: &[(f64, Tolerance)]) -> Span {
    let mut min = 0.0;
    let mut max = 0.0;
    for (nominal, tolerance) in parts {
        let (low, high) = band(*nominal, *tolerance);
        min += 1.0 / low.max(1e-18);
        max += 1.0 / high.max(1e-18);
    }
    Span::new(1.0 / min, 1.0 / max)
}

fn corners(
    r_a: f64,
    tol_a: Tolerance,
    r_b: f64,
    tol_b: Tolerance,
    a: Span,
    b: Span,
) -> Vec<(f64, f64, f64, f64)> {
    let (ra_low, ra_high) = band(r_a, tol_a);
    let (rb_low, rb_high) = band(r_b, tol_b);
    let mut out = Vec::new();
    for ra in [ra_low, ra_high] {
        for rb in [rb_low, rb_high] {
            for va in [a.min, a.max] {
                for vb in [b.min, b.max] {
                    out.push((ra, rb, va, vb));
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn power_uses_the_low_resistance_and_the_widest_voltage() {
        let power = worst_power(
            Span::point(0.0),
            Span::new(3.0, 4.2),
            10_000.0,
            Tolerance::Relative(0.01),
        );
        let expected = 4.2 * 4.2 / 9_900.0;
        assert!((power - expected).abs() < 1e-9);
    }

    #[test]
    fn equal_divider_sits_around_half_the_rail() {
        let span = divider_span(
            Span::point(3.3),
            Span::point(0.0),
            10_000.0,
            Tolerance::Relative(0.01),
            10_000.0,
            Tolerance::Relative(0.01),
        );
        assert!(span.min < 1.65 && span.max > 1.65);
        assert!(span.min > 1.63 && span.max < 1.67);
    }

    #[test]
    fn two_equal_resistors_halve_in_parallel_and_double_in_series() {
        let parallel = parallel_resistance(&[
            (10_000.0, Tolerance::Relative(0.0)),
            (10_000.0, Tolerance::Relative(0.0)),
        ]);
        let series = series_resistance(&[
            (10_000.0, Tolerance::Relative(0.0)),
            (10_000.0, Tolerance::Relative(0.0)),
        ]);
        assert!((parallel.min - 5_000.0).abs() < 1e-6);
        assert!((series.min - 20_000.0).abs() < 1e-6);
    }

    #[test]
    fn tau_is_the_product_of_the_bands() {
        let span = tau_span(
            10_000.0,
            Tolerance::Relative(0.0),
            100e-9,
            Tolerance::Relative(0.0),
        );
        assert!((span.min - 0.001).abs() < 1e-12);
    }
}
