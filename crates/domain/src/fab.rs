//! Fabrication profiles. One shape for the built-in tables and a JSON file.
//!
//! Lengths are millimetres. A missing optional value means that check stays
//! as the project left it. Track rows are required.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

const OZ_MM: f64 = 0.035;
const OZ_TOLERANCE: f64 = 0.05;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FabProfile {
    pub name: String,
    pub tracks: Vec<TrackBand>,
    #[serde(default)]
    pub annular: Vec<AnnularBand>,
    #[serde(default)]
    pub via: Option<ViaLimits>,
    #[serde(default)]
    pub pth: Option<DrillLimit>,
    #[serde(default)]
    pub npth: Option<NpthLimits>,
    #[serde(default)]
    pub hole_spacing: Option<HoleSpacing>,
    #[serde(default)]
    pub hole_to_copper: Option<HoleCopper>,
    #[serde(default)]
    pub copper_to_edge_mm: Option<f64>,
    #[serde(default)]
    pub mask: Vec<MaskBand>,
    #[serde(default)]
    pub smd: Vec<SmdBand>,
    #[serde(default)]
    pub silk: Option<SilkLimits>,
    #[serde(default)]
    pub plated_slot_mm: Option<f64>,
    #[serde(default)]
    pub npth_slot_mm: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TrackBand {
    pub min_layers: u32,
    #[serde(default)]
    pub max_layers: Option<u32>,
    pub min_oz: f64,
    pub width_mm: f64,
    pub space_mm: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AnnularBand {
    pub min_layers: u32,
    #[serde(default)]
    pub max_layers: Option<u32>,
    pub min_oz: f64,
    pub fail_mm: f64,
    #[serde(default)]
    pub warn_mm: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ViaLimits {
    pub drill_fail_mm: f64,
    #[serde(default)]
    pub drill_warn_mm: Option<f64>,
    /// Pad diameter minus the drill. KiCad's annular width is half of this.
    pub extra_fail_mm: f64,
    #[serde(default)]
    pub extra_warn_mm: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DrillLimit {
    pub drill_fail_mm: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NpthLimits {
    pub drill_fail_mm: f64,
    #[serde(default)]
    pub annular_warn_mm: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HoleSpacing {
    pub via_mm: f64,
    pub pth_mm: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HoleCopper {
    pub via_mm: f64,
    pub npth_mm: f64,
    pub pth_outer_fail_mm: f64,
    #[serde(default)]
    pub pth_outer_warn_mm: Option<f64>,
    pub pth_inner_mm: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MaskBand {
    pub colors: Vec<String>,
    #[serde(default)]
    pub max_oz: Option<f64>,
    pub bridge_mm: f64,
    #[serde(default)]
    pub to_copper_mm: Option<f64>,
    #[serde(default)]
    pub pad_spacing_mm: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SmdBand {
    #[serde(default)]
    pub max_oz: Option<f64>,
    pub pad_spacing_mm: f64,
    pub pad_to_track_mm: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SilkLimits {
    pub stroke_mm: f64,
    pub height_mm: f64,
    #[serde(default)]
    pub to_pad_mm: Option<f64>,
}

/// The one row that applies to this board. Warnings stay here for the DFM
/// test. KiCad rules use the fail values, and silkscreen stays a warning.
#[derive(Debug, Clone, PartialEq)]
pub struct Limits {
    pub name: String,
    pub layers: u32,
    pub outer_oz: f64,
    pub mask: String,
    pub track_width_mm: f64,
    pub track_space_mm: f64,
    pub annular_fail_mm: Option<f64>,
    pub annular_warn_mm: Option<f64>,
    pub via_drill_fail_mm: Option<f64>,
    pub via_drill_warn_mm: Option<f64>,
    pub via_extra_fail_mm: Option<f64>,
    pub via_extra_warn_mm: Option<f64>,
    pub pth_drill_fail_mm: Option<f64>,
    pub npth_drill_fail_mm: Option<f64>,
    pub npth_annular_warn_mm: Option<f64>,
    pub via_hole_spacing_mm: Option<f64>,
    pub pth_hole_spacing_mm: Option<f64>,
    pub via_hole_copper_mm: Option<f64>,
    pub npth_hole_copper_mm: Option<f64>,
    pub pth_hole_copper_outer_fail_mm: Option<f64>,
    pub pth_hole_copper_outer_warn_mm: Option<f64>,
    pub pth_hole_copper_inner_mm: Option<f64>,
    pub copper_to_edge_mm: Option<f64>,
    pub mask_bridge_mm: Option<f64>,
    pub mask_to_copper_mm: Option<f64>,
    pub smd_pad_spacing_mm: Option<f64>,
    pub smd_pad_to_track_mm: Option<f64>,
    pub silk_stroke_mm: Option<f64>,
    pub silk_height_mm: Option<f64>,
    pub silk_to_pad_mm: Option<f64>,
    pub plated_slot_mm: Option<f64>,
    pub npth_slot_mm: Option<f64>,
}

impl Limits {
    /// KiCad measures the ring, which is half the extra pad diameter.
    pub fn via_annular_mm(&self) -> Option<f64> {
        self.via_extra_fail_mm.map(|extra| extra / 2.0)
    }

    pub fn via_diameter_mm(&self) -> Option<f64> {
        Some(self.via_drill_fail_mm? + self.via_extra_fail_mm?)
    }

    pub fn min_drill_mm(&self) -> Option<f64> {
        match (self.pth_drill_fail_mm, self.via_drill_fail_mm) {
            (Some(pth), Some(via)) => Some(pth.min(via)),
            (Some(value), None) | (None, Some(value)) => Some(value),
            (None, None) => None,
        }
    }

    pub fn min_hole_spacing_mm(&self) -> Option<f64> {
        match (self.via_hole_spacing_mm, self.pth_hole_spacing_mm) {
            (Some(via), Some(pth)) => Some(via.min(pth)),
            (Some(value), None) | (None, Some(value)) => Some(value),
            (None, None) => None,
        }
    }

    pub fn min_hole_copper_mm(&self) -> Option<f64> {
        let values = [
            self.via_hole_copper_mm,
            self.npth_hole_copper_mm,
            self.pth_hole_copper_outer_fail_mm,
            self.pth_hole_copper_inner_mm,
        ];
        values.into_iter().flatten().reduce(f64::min)
    }
}

pub fn jlcpcb() -> FabProfile {
    profile(JLCPCB)
}

pub fn pcbway() -> FabProfile {
    profile(PCBWAY)
}

pub fn from_json(text: &str) -> Result<FabProfile> {
    let profile: FabProfile = serde_json::from_str(text).context("profile JSON")?;
    profile.check()?;
    Ok(profile)
}

impl FabProfile {
    fn check(&self) -> Result<()> {
        if self.name.is_empty()
            || !self
                .name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-')
        {
            bail!("profile name must be letters, digits, or a hyphen");
        }
        if self.tracks.is_empty() {
            bail!("profile tracks must not be empty");
        }
        Ok(())
    }
}

/// `outer_mm` is the thicker of the two outer copper layers.
pub fn limits(
    profile: &FabProfile,
    copper_layers: u32,
    outer_mm: f64,
    mask: &str,
) -> Result<Limits> {
    profile.check()?;
    let layers = copper_layers.max(1);
    let measured = outer_mm / OZ_MM;
    let track_rows: Vec<&TrackBand> = profile
        .tracks
        .iter()
        .filter(|row| covers(row.min_layers, row.max_layers, layers))
        .collect();
    if track_rows.is_empty() {
        bail!("no track limit for {layers} layers");
    }
    let track_oz = snap_up(
        measured,
        &track_rows.iter().map(|row| row.min_oz).collect::<Vec<_>>(),
    );
    let track = track_rows
        .into_iter()
        .find(|row| (row.min_oz - track_oz).abs() < 1e-6)
        .context("track row")?;

    let annular = pick_oz(
        &profile.annular,
        layers,
        measured,
        |row| (row.min_layers, row.max_layers),
        |row| row.min_oz,
    );
    let smd = profile.smd.iter().find(|row| under(row.max_oz, track_oz));
    let mask_row = profile
        .mask
        .iter()
        .find(|row| row.colors.iter().any(|color| color == mask) && under(row.max_oz, track_oz));
    if !profile.mask.is_empty() && mask_row.is_none() {
        bail!("no mask limit for {mask}");
    }

    Ok(Limits {
        name: profile.name.clone(),
        layers,
        outer_oz: track_oz,
        mask: mask.to_string(),
        track_width_mm: track.width_mm,
        track_space_mm: track.space_mm,
        annular_fail_mm: annular.map(|row| row.fail_mm),
        annular_warn_mm: annular.and_then(|row| row.warn_mm),
        via_drill_fail_mm: profile.via.as_ref().map(|via| via.drill_fail_mm),
        via_drill_warn_mm: profile.via.as_ref().and_then(|via| via.drill_warn_mm),
        via_extra_fail_mm: profile.via.as_ref().map(|via| via.extra_fail_mm),
        via_extra_warn_mm: profile.via.as_ref().and_then(|via| via.extra_warn_mm),
        pth_drill_fail_mm: profile.pth.as_ref().map(|pth| pth.drill_fail_mm),
        npth_drill_fail_mm: profile.npth.as_ref().map(|npth| npth.drill_fail_mm),
        npth_annular_warn_mm: profile.npth.as_ref().and_then(|npth| npth.annular_warn_mm),
        via_hole_spacing_mm: profile.hole_spacing.as_ref().map(|hole| hole.via_mm),
        pth_hole_spacing_mm: profile.hole_spacing.as_ref().map(|hole| hole.pth_mm),
        via_hole_copper_mm: profile.hole_to_copper.as_ref().map(|hole| hole.via_mm),
        npth_hole_copper_mm: profile.hole_to_copper.as_ref().map(|hole| hole.npth_mm),
        pth_hole_copper_outer_fail_mm: profile
            .hole_to_copper
            .as_ref()
            .map(|hole| hole.pth_outer_fail_mm),
        pth_hole_copper_outer_warn_mm: profile
            .hole_to_copper
            .as_ref()
            .and_then(|hole| hole.pth_outer_warn_mm),
        pth_hole_copper_inner_mm: profile
            .hole_to_copper
            .as_ref()
            .map(|hole| hole.pth_inner_mm),
        copper_to_edge_mm: profile.copper_to_edge_mm,
        mask_bridge_mm: mask_row.map(|row| row.bridge_mm),
        mask_to_copper_mm: mask_row.and_then(|row| row.to_copper_mm),
        smd_pad_spacing_mm: smd
            .map(|row| row.pad_spacing_mm)
            .or_else(|| mask_row.and_then(|row| row.pad_spacing_mm)),
        smd_pad_to_track_mm: smd.map(|row| row.pad_to_track_mm),
        silk_stroke_mm: profile.silk.as_ref().map(|silk| silk.stroke_mm),
        silk_height_mm: profile.silk.as_ref().map(|silk| silk.height_mm),
        silk_to_pad_mm: profile.silk.as_ref().and_then(|silk| silk.to_pad_mm),
        plated_slot_mm: profile.plated_slot_mm,
        npth_slot_mm: profile.npth_slot_mm,
    })
}

fn covers(min_layers: u32, max_layers: Option<u32>, layers: u32) -> bool {
    layers >= min_layers && max_layers.is_none_or(|max| layers <= max)
}

fn under(max_oz: Option<f64>, oz: f64) -> bool {
    max_oz.is_none_or(|max| oz < max - 1e-6)
}

fn pick_oz<'a, T>(
    rows: &'a [T],
    layers: u32,
    measured_oz: f64,
    layers_of: impl Fn(&T) -> (u32, Option<u32>),
    min_oz_of: impl Fn(&T) -> f64,
) -> Option<&'a T> {
    let matched: Vec<&T> = rows
        .iter()
        .filter(|row| {
            let (min_layers, max_layers) = layers_of(row);
            covers(min_layers, max_layers, layers)
        })
        .collect();
    if matched.is_empty() {
        return None;
    }
    let oz = snap_up(
        measured_oz,
        &matched.iter().map(|row| min_oz_of(row)).collect::<Vec<_>>(),
    );
    matched
        .into_iter()
        .find(|row| (min_oz_of(row) - oz).abs() < 1e-6)
}

/// Smallest published step that is still at least the measured weight.
/// A value past every step uses the heaviest step. A few hundredths of an
/// ounce still count as the step below, so 0.036 mm stays 1 oz.
fn snap_up(measured: f64, steps: &[f64]) -> f64 {
    let mut above: Option<f64> = None;
    for step in steps {
        if *step + OZ_TOLERANCE >= measured && above.is_none_or(|best| *step < best) {
            above = Some(*step);
        }
    }
    above.unwrap_or_else(|| steps.iter().copied().fold(f64::NEG_INFINITY, f64::max))
}

#[derive(Clone, Copy)]
struct BuiltIn {
    name: &'static str,
    tracks: &'static [TrackBand],
    annular: &'static [AnnularBand],
    via: ViaLimits,
    pth_drill: f64,
    npth: NpthLimits,
    hole_spacing: HoleSpacing,
    hole_to_copper: HoleCopper,
    copper_to_edge: f64,
    smd: &'static [SmdBand],
    silk: SilkLimits,
    plated_slot: f64,
    npth_slot: f64,
}

const JLCPCB_COLORS: &[&str] = &["green", "red", "yellow", "blue", "purple"];

fn colors(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| (*name).to_string()).collect()
}

const JLCPCB: BuiltIn = BuiltIn {
    name: "jlcpcb",
    tracks: &[
        TrackBand {
            min_layers: 1,
            max_layers: Some(2),
            min_oz: 1.0,
            width_mm: 0.10,
            space_mm: 0.10,
        },
        TrackBand {
            min_layers: 3,
            max_layers: None,
            min_oz: 1.0,
            width_mm: 0.09,
            space_mm: 0.09,
        },
        TrackBand {
            min_layers: 1,
            max_layers: Some(2),
            min_oz: 2.0,
            width_mm: 0.16,
            space_mm: 0.16,
        },
        TrackBand {
            min_layers: 3,
            max_layers: None,
            min_oz: 2.0,
            width_mm: 0.15,
            space_mm: 0.15,
        },
        TrackBand {
            min_layers: 1,
            max_layers: Some(2),
            min_oz: 2.5,
            width_mm: 0.20,
            space_mm: 0.20,
        },
        TrackBand {
            min_layers: 1,
            max_layers: Some(2),
            min_oz: 3.5,
            width_mm: 0.25,
            space_mm: 0.25,
        },
        TrackBand {
            min_layers: 1,
            max_layers: Some(2),
            min_oz: 4.5,
            width_mm: 0.30,
            space_mm: 0.30,
        },
    ],
    annular: &[
        AnnularBand {
            min_layers: 1,
            max_layers: Some(2),
            min_oz: 1.0,
            fail_mm: 0.18,
            warn_mm: Some(0.25),
        },
        AnnularBand {
            min_layers: 1,
            max_layers: Some(2),
            min_oz: 2.0,
            fail_mm: 0.254,
            warn_mm: None,
        },
        AnnularBand {
            min_layers: 3,
            max_layers: None,
            min_oz: 1.0,
            fail_mm: 0.15,
            warn_mm: Some(0.20),
        },
        AnnularBand {
            min_layers: 3,
            max_layers: None,
            min_oz: 2.0,
            fail_mm: 0.254,
            warn_mm: None,
        },
    ],
    via: ViaLimits {
        drill_fail_mm: 0.15,
        drill_warn_mm: Some(0.20),
        extra_fail_mm: 0.10,
        extra_warn_mm: Some(0.15),
    },
    pth_drill: 0.15,
    npth: NpthLimits {
        drill_fail_mm: 0.50,
        annular_warn_mm: Some(0.45),
    },
    hole_spacing: HoleSpacing {
        via_mm: 0.20,
        pth_mm: 0.45,
    },
    hole_to_copper: HoleCopper {
        via_mm: 0.20,
        npth_mm: 0.20,
        pth_outer_fail_mm: 0.28,
        pth_outer_warn_mm: Some(0.35),
        pth_inner_mm: 0.30,
    },
    copper_to_edge: 0.20,
    smd: &[
        SmdBand {
            max_oz: Some(2.0),
            pad_spacing_mm: 0.15,
            pad_to_track_mm: 0.10,
        },
        SmdBand {
            max_oz: None,
            pad_spacing_mm: 0.15,
            pad_to_track_mm: 0.15,
        },
    ],
    silk: SilkLimits {
        stroke_mm: 0.15,
        height_mm: 1.0,
        to_pad_mm: Some(0.15),
    },
    plated_slot: 0.50,
    npth_slot: 1.0,
};

const PCBWAY: BuiltIn = BuiltIn {
    name: "pcbway",
    tracks: &[
        TrackBand {
            min_layers: 1,
            max_layers: None,
            min_oz: 1.0,
            width_mm: 0.10,
            space_mm: 0.10,
        },
        TrackBand {
            min_layers: 1,
            max_layers: None,
            min_oz: 2.0,
            width_mm: 0.127,
            space_mm: 0.152,
        },
        TrackBand {
            min_layers: 1,
            max_layers: None,
            min_oz: 3.0,
            width_mm: 0.152,
            space_mm: 0.229,
        },
    ],
    annular: &[
        AnnularBand {
            min_layers: 1,
            max_layers: None,
            min_oz: 1.0,
            fail_mm: 0.203,
            warn_mm: None,
        },
        AnnularBand {
            min_layers: 1,
            max_layers: None,
            min_oz: 2.0,
            fail_mm: 0.254,
            warn_mm: None,
        },
    ],
    via: ViaLimits {
        drill_fail_mm: 0.15,
        drill_warn_mm: Some(0.20),
        extra_fail_mm: 0.30,
        extra_warn_mm: None,
    },
    pth_drill: 0.15,
    npth: NpthLimits {
        drill_fail_mm: 0.40,
        annular_warn_mm: None,
    },
    hole_spacing: HoleSpacing {
        via_mm: 0.20,
        pth_mm: 0.35,
    },
    hole_to_copper: HoleCopper {
        via_mm: 0.15,
        npth_mm: 0.15,
        pth_outer_fail_mm: 0.15,
        pth_outer_warn_mm: None,
        pth_inner_mm: 0.15,
    },
    copper_to_edge: 0.20,
    smd: &[],
    silk: SilkLimits {
        stroke_mm: 0.15,
        height_mm: 0.80,
        to_pad_mm: None,
    },
    plated_slot: 0.50,
    npth_slot: 0.80,
};

fn jlcpcb_mask() -> Vec<MaskBand> {
    let mut rows = vec![
        MaskBand {
            colors: colors(&["black", "white"]),
            max_oz: Some(2.0),
            bridge_mm: 0.13,
            to_copper_mm: Some(0.09),
            pad_spacing_mm: None,
        },
        MaskBand {
            colors: colors(JLCPCB_COLORS),
            max_oz: Some(2.0),
            bridge_mm: 0.10,
            to_copper_mm: Some(0.09),
            pad_spacing_mm: None,
        },
    ];
    let mut heavy = colors(&["black", "white"]);
    heavy.extend(colors(JLCPCB_COLORS));
    rows.push(MaskBand {
        colors: heavy,
        max_oz: None,
        bridge_mm: 0.20,
        to_copper_mm: Some(0.09),
        pad_spacing_mm: None,
    });
    rows
}

fn pcbway_mask() -> Vec<MaskBand> {
    vec![
        MaskBand {
            colors: colors(&["green"]),
            max_oz: None,
            bridge_mm: 0.076,
            to_copper_mm: None,
            pad_spacing_mm: Some(0.20),
        },
        MaskBand {
            colors: colors(&["red", "yellow", "blue", "white", "black", "purple"]),
            max_oz: None,
            bridge_mm: 0.102,
            to_copper_mm: None,
            pad_spacing_mm: Some(0.22),
        },
    ]
}

/// The mask tables are built at runtime because their color lists are owned.
/// Track, annular, and hole tables above stay `const`.
fn profile(built: BuiltIn) -> FabProfile {
    let mut profile = FabProfile {
        name: built.name.to_string(),
        tracks: built.tracks.to_vec(),
        annular: built.annular.to_vec(),
        via: Some(built.via),
        pth: Some(DrillLimit {
            drill_fail_mm: built.pth_drill,
        }),
        npth: Some(built.npth),
        hole_spacing: Some(built.hole_spacing),
        hole_to_copper: Some(built.hole_to_copper),
        copper_to_edge_mm: Some(built.copper_to_edge),
        mask: Vec::new(),
        smd: built.smd.to_vec(),
        silk: Some(built.silk),
        plated_slot_mm: Some(built.plated_slot),
        npth_slot_mm: Some(built.npth_slot),
    };
    profile.mask = if built.name == "jlcpcb" {
        jlcpcb_mask()
    } else {
        pcbway_mask()
    };
    profile.check().expect("built-in profile");
    profile
}

#[cfg(test)]
mod tests {
    use super::*;

    fn near(left: f64, right: f64) {
        assert!((left - right).abs() < 1e-9, "{left} != {right}");
    }

    #[test]
    fn jlcpcb_track_rows_follow_layers_and_copper() {
        let profile = jlcpcb();
        let two = limits(&profile, 2, 0.035, "green").unwrap();
        near(two.track_width_mm, 0.10);
        near(two.outer_oz, 1.0);
        near(two.annular_fail_mm.unwrap(), 0.18);
        near(two.annular_warn_mm.unwrap(), 0.25);
        near(two.mask_bridge_mm.unwrap(), 0.10);
        near(two.smd_pad_to_track_mm.unwrap(), 0.10);

        let four = limits(&profile, 4, 0.035, "black").unwrap();
        near(four.track_width_mm, 0.09);
        near(four.annular_fail_mm.unwrap(), 0.15);
        near(four.mask_bridge_mm.unwrap(), 0.13);

        let heavy = limits(&profile, 2, 0.070, "green").unwrap();
        near(heavy.track_width_mm, 0.16);
        near(heavy.annular_fail_mm.unwrap(), 0.254);
        assert!(heavy.annular_warn_mm.is_none());
        near(heavy.mask_bridge_mm.unwrap(), 0.20);
        near(heavy.smd_pad_to_track_mm.unwrap(), 0.15);
        near(heavy.via_annular_mm().unwrap(), 0.05);
    }

    #[test]
    fn a_hair_over_one_ounce_stays_on_the_one_ounce_row() {
        let row = limits(&jlcpcb(), 2, 0.036, "green").unwrap();
        near(row.outer_oz, 1.0);
        near(row.track_width_mm, 0.10);
    }

    #[test]
    fn pcbway_uses_the_standard_review_floors() {
        let row = limits(&pcbway(), 4, 0.070, "white").unwrap();
        near(row.track_width_mm, 0.127);
        near(row.track_space_mm, 0.152);
        near(row.annular_fail_mm.unwrap(), 0.254);
        near(row.via_annular_mm().unwrap(), 0.15);
        near(row.mask_bridge_mm.unwrap(), 0.102);
        near(row.smd_pad_spacing_mm.unwrap(), 0.22);
        assert!(row.silk_to_pad_mm.is_none());
        near(row.npth_slot_mm.unwrap(), 0.80);
    }

    #[test]
    fn json_uses_the_same_profile_and_rejects_a_foreign_key() {
        let text = r#"{
            "name": "acme",
            "tracks": [{ "minLayers": 1, "maxLayers": 2, "minOz": 1, "widthMm": 0.2, "spaceMm": 0.2 }]
        }"#;
        let profile = from_json(text).unwrap();
        near(
            limits(&profile, 2, 0.035, "green").unwrap().track_width_mm,
            0.2,
        );
        let err = limits(&profile, 4, 0.035, "green").unwrap_err();
        assert!(err.to_string().contains("4 layers"), "{err}");
        assert!(from_json(r#"{"name":"acme","tracks":[],"extra":1}"#).is_err());
        assert!(from_json(
            r#"{"name":"ac me","tracks":[{"minLayers":1,"minOz":1,"widthMm":0.2,"spaceMm":0.2}]}"#
        )
        .is_err());
    }
}
