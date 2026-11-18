use crate::parts::Dimension;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Resistor,
    Capacitor,
    Inductor,
}

impl Kind {
    pub fn value_key(self) -> &'static str {
        match self {
            Self::Resistor => "resistance",
            Self::Capacitor => "capacitance",
            Self::Inductor => "inductance",
        }
    }

    pub fn dimension(self) -> Dimension {
        match self {
            Self::Resistor => Dimension::Ohm,
            Self::Capacitor => Dimension::Farad,
            Self::Inductor => Dimension::Henry,
        }
    }
}

/// `R12`, `C3`, and `L1`. Packs such as `RN2` are not two-terminal parts.
pub fn passive_kind(reference: &str) -> Option<Kind> {
    let (kind, rest) = if let Some(rest) = reference.strip_prefix('R') {
        (Kind::Resistor, rest)
    } else if let Some(rest) = reference.strip_prefix('C') {
        (Kind::Capacitor, rest)
    } else if let Some(rest) = reference.strip_prefix('L') {
        (Kind::Inductor, rest)
    } else {
        return None;
    };
    if !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()) {
        Some(kind)
    } else {
        None
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Tolerance {
    /// Fraction of the nominal, so `0.01` is ±1%.
    Relative(f64),
    /// Offset in the same SI unit as the nominal.
    Absolute(f64),
}

pub fn value_within(schematic: f64, nominal: f64, tolerance: Tolerance) -> bool {
    let (low, high) = band(nominal, tolerance);
    let slack = (nominal.abs() * 1e-9).max(1e-18);
    schematic + slack >= low && schematic - slack <= high
}

pub fn band(nominal: f64, tolerance: Tolerance) -> (f64, f64) {
    let delta = match tolerance {
        Tolerance::Relative(fraction) => nominal.abs() * fraction,
        Tolerance::Absolute(amount) => amount.abs(),
    };
    (nominal - delta, nominal + delta)
}

pub fn format_si(value: f64, dimension: Dimension) -> String {
    let (scale, prefix) = if value.abs() >= 1e6 {
        (1e6, "M")
    } else if value.abs() >= 1e3 {
        (1e3, "k")
    } else if value.abs() >= 1.0 || value == 0.0 {
        (1.0, "")
    } else if value.abs() >= 1e-3 {
        (1e-3, "m")
    } else if value.abs() >= 1e-6 {
        (1e-6, "u")
    } else if value.abs() >= 1e-9 {
        (1e-9, "n")
    } else {
        (1e-12, "p")
    };
    let unit = match dimension {
        Dimension::Ohm => "ohm",
        Dimension::Farad => "F",
        Dimension::Henry => "H",
        Dimension::Volt => "V",
        Dimension::Watt => "W",
        Dimension::Ampere => "A",
        Dimension::Percent => "%",
    };
    let shown = value / scale;
    let text = format!("{shown:.4}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    format!("{text}{prefix}{unit}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packs_are_not_single_passives() {
        assert_eq!(passive_kind("R12"), Some(Kind::Resistor));
        assert_eq!(passive_kind("C3"), Some(Kind::Capacitor));
        assert_eq!(passive_kind("L1"), Some(Kind::Inductor));
        assert_eq!(passive_kind("RN2"), None);
        assert_eq!(passive_kind("R12A"), None);
    }

    #[test]
    fn one_percent_band_accepts_the_nominal() {
        assert!(value_within(10_000.0, 10_000.0, Tolerance::Relative(0.01)));
        assert!(value_within(10_100.0, 10_000.0, Tolerance::Relative(0.01)));
        assert!(!value_within(4_700.0, 10_000.0, Tolerance::Relative(0.01)));
        assert!(value_within(
            1.5e-12,
            1.5e-12,
            Tolerance::Absolute(0.25e-12)
        ));
        assert!(!value_within(
            1.0e-12,
            1.5e-12,
            Tolerance::Absolute(0.25e-12)
        ));
    }
}
