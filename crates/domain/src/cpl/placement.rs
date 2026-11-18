//! Geometria CPL taka jak w `fabrication.py` wtyczki kicad-jlcpcb-tools.
//!
//! KiCad w pliku pozycji zapisuje surowy kąt footprintu, także na spodzie,
//! i oś Y skierowaną w górę. Wtyczka, zanim doda korektę z bazy, zamienia
//! kąt spodu na `(180° − kąt) mod 360`. Ta sama liczba wychodzi z narzędzia,
//! dla którego tabelę napisano (JLCKicadTools): tam korekta na spodzie jest
//! odejmowana, a potem kąt przechodzi przez `(-kąt + 180) mod 360`.
//!
//! Przesunięcie z reguły jest w układzie footprintu (Y w dół, tak jak
//! współrzędne wewnętrzne KiCada w `reposition`). Do pliku pozycji trafia
//! już po obrocie o kąt sprzed korekty i po odwróceniu osi Y.

/// Różnica dwóch kątów w stopniach, w zakresie 0…180.
pub(crate) fn angular_separation(a: f64, b: f64) -> f64 {
    let mut delta = (a - b).rem_euclid(360.0);
    if delta > 180.0 {
        delta = 360.0 - delta;
    }
    delta
}

/// Kąt zapisany do CPL.
///
/// `correction == None` oznacza brak reguły. Wierzch zostaje wtedy przy kącie
/// KiCada, razem ze znakiem. Spód zawsze dostaje odbicie.
pub(crate) fn jlc_rotation(kicad_deg: f64, correction: Option<i64>, bottom: bool) -> f64 {
    let base = if bottom {
        (180.0 - kicad_deg).rem_euclid(360.0)
    } else {
        kicad_deg
    };
    match correction {
        Some(delta) => (base + delta as f64).rem_euclid(360.0),
        None if bottom => base.rem_euclid(360.0),
        None => base,
    }
}

/// Przesunięcie `(ox, oy)` w mm z układu footprintu na deltę pliku pozycji.
pub(crate) fn position_delta(ox: f64, oy: f64, kicad_deg: f64, bottom: bool) -> (f64, f64) {
    let theta = if bottom {
        (180.0 - kicad_deg).rem_euclid(360.0)
    } else {
        kicad_deg.rem_euclid(360.0)
    };
    let (sin, cos) = theta.to_radians().sin_cos();
    let mut dx = ox * cos + oy * sin;
    let dy = -ox * sin + oy * cos;
    if bottom {
        dx = -dx;
    }
    (dx, -dy)
}

pub(crate) fn format_mm(value: f64) -> String {
    let value = if value.abs() < 5e-7 { 0.0 } else { value };
    format!("{value:.6}")
}

pub(crate) fn format_number(value: f64) -> String {
    if !value.is_finite() || value.abs() < 5e-7 {
        return "0".to_string();
    }
    let text = format!("{value:.6}");
    let negative = text.starts_with('-');
    let digits = text.trim_start_matches('-');
    let trimmed = digits.trim_end_matches('0').trim_end_matches('.');
    if trimmed.is_empty() {
        "0".to_string()
    } else if negative {
        format!("-{trimmed}")
    } else {
        trimmed.to_string()
    }
}

pub(crate) const MM_EPSILON: f64 = 5e-7;
pub(crate) const DEG_EPSILON: f64 = 1e-4;

#[cfg(test)]
mod tests {
    use super::*;

    fn matthew_lai(kicad: f64, correction: f64, bottom: bool) -> f64 {
        if bottom {
            let rotation = (kicad - correction).rem_euclid(360.0);
            (-rotation + 180.0).rem_euclid(360.0)
        } else {
            (kicad + correction).rem_euclid(360.0)
        }
    }

    fn assert_mm(got: f64, want: f64) {
        assert!(
            (got - want).abs() < 1e-6,
            "{got} != {want} (difference {})",
            (got - want).abs()
        );
    }

    #[test]
    fn matches_jlckicadtools_when_a_rule_applies() {
        let angles = [
            0.0, 30.0, 45.0, 90.0, -90.0, 180.0, -180.0, 270.0, 359.0, 10.5, -10.5,
        ];
        let corrections = [-270, -180, -90, 0, 90, 180, 270, 360, -360];
        for &angle in &angles {
            for &correction in &corrections {
                for bottom in [false, true] {
                    let got = jlc_rotation(angle, Some(correction), bottom);
                    let want = matthew_lai(angle, correction as f64, bottom);
                    assert!(
                        angular_separation(got, want) < 1e-6,
                        "angle {angle}, correction {correction}, bottom {bottom}: {got} vs {want}"
                    );
                }
            }
        }
    }

    #[test]
    fn bottom_is_mirrored_without_a_rule() {
        assert_eq!(jlc_rotation(180.0, None, true), 0.0);
        assert_eq!(jlc_rotation(0.0, None, true), 180.0);
        assert_eq!(jlc_rotation(-90.0, None, true), 270.0);
        assert_eq!(jlc_rotation(90.0, None, true), 90.0);
        assert_eq!(jlc_rotation(-90.0, None, false), -90.0);
        assert_eq!(jlc_rotation(-90.0, Some(0), false), 270.0);
    }

    #[test]
    fn offset_follows_the_plugin_reposition_matrix() {
        let (dx, dy) = position_delta(2.54, 0.635, 0.0, false);
        assert_mm(dx, 2.54);
        assert_mm(dy, -0.635);

        let (dx, dy) = position_delta(2.54, 0.635, 90.0, false);
        assert_mm(dx, 0.635);
        assert_mm(dy, 2.54);

        let (dx, dy) = position_delta(2.54, 0.635, 0.0, true);
        assert_mm(dx, 2.54);
        assert_mm(dy, 0.635);

        let (dx, dy) = position_delta(2.54, 0.635, 90.0, true);
        assert_mm(dx, -0.635);
        assert_mm(dy, 2.54);

        let (dx, dy) = position_delta(0.0, 0.0, 33.0, true);
        assert_mm(dx, 0.0);
        assert_mm(dy, 0.0);
    }

    #[test]
    fn equivalent_angles_have_no_separation() {
        assert!(angular_separation(-90.0, 270.0) < DEG_EPSILON);
        assert!(angular_separation(0.0, 360.0) < DEG_EPSILON);
        assert!(angular_separation(180.0, 0.0) > 90.0);
    }
}
