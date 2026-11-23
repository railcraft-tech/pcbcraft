//! JLCPCB process limits from the published capability tables.
//!
//! A value below the documented minimum fails. A value between that minimum
//! and a documented recommendation is a warning. One published number, described
//! as a capability minimum, has no warning band. Silk and package-spacing
//! numbers are warnings: the board is still fabricated.

use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result};
use pcbcraft_domain::fab::{self, Limits};
use pcbcraft_domain::geometry::{
    boundary_gap, boundary_segments, contains, filled_separation, outline_window, BoardGeometry,
    FootprintName, Outline, PadForm, Point, Shape, Source, Window,
};
use serde::Deserialize;
use serde_json::{json, Value};

use super::scheduler::TestContext;
use super::validator::{spec_schema, Job, Line, ValidatorInterface};
use crate::ports::TestStatus;

const SEARCH: f64 = 0.5;
const CONTACT: f64 = 0.005;

pub struct JLCPCBDFM;

#[derive(Debug, Deserialize)]
struct Spec {
    project: String,
    #[serde(default = "green", rename = "maskColor")]
    mask_color: String,
    #[serde(default)]
    rails: Vec<String>,
    #[serde(default, rename = "allowOverhang")]
    allow_overhang: Vec<String>,
    #[serde(default, rename = "outerCopperOz")]
    outer_copper_oz: Option<f64>,
    #[serde(default, rename = "innerCopperOz")]
    inner_copper_oz: Option<f64>,
}

fn green() -> String {
    "green".into()
}

#[derive(Clone, Copy)]
struct Band {
    fail_below: Option<f64>,
    warn_below: Option<f64>,
}

#[derive(Clone)]
struct Site {
    distance: f64,
    label: String,
}

pub fn evaluate(board: &BoardGeometry, spec: &Value) -> Result<Vec<Line>> {
    let spec: Spec = serde_json::from_value(spec.clone()).context("spec JLCPCBDFM")?;
    report(board, &spec)
}

impl ValidatorInterface for JLCPCBDFM {
    fn type_name(&self) -> &'static str {
        "JLCPCBDFM"
    }

    fn schema(&self) -> Value {
        spec_schema(
            json!({
                "project": { "type": "string", "minLength": 1 },
                "maskColor": { "type": "string", "enum": ["green", "red", "yellow", "blue", "white", "black", "purple"] },
                "rails": {
                    "type": "array",
                    "items": { "type": "string", "enum": ["top", "bottom", "left", "right"] }
                },
                "allowOverhang": { "type": "array", "items": { "type": "string" } },
                "outerCopperOz": { "type": "number", "exclusiveMinimum": 0 },
                "innerCopperOz": { "type": "number", "exclusiveMinimum": 0 }
            }),
            &["project"],
        )
    }

    fn run(&self, file_name: &str, spec: &Value, ctx: &TestContext<'_>) -> Result<Vec<Job>> {
        let parsed: Spec = serde_json::from_value(spec.clone()).context("spec JLCPCBDFM")?;
        let board = ctx.boards.load(Path::new(&parsed.project))?;
        Ok(super::validator::ready_jobs(
            file_name,
            report(&board, &parsed)?,
        ))
    }
}

fn floors(board: &BoardGeometry, spec: &Spec) -> Result<Limits> {
    let outer_oz = spec
        .outer_copper_oz
        .unwrap_or_else(|| ounces(board.outer_copper_mm, 1.0));
    let _inner_oz = spec.inner_copper_oz;
    fab::limits(
        &fab::jlcpcb(),
        board.copper_layers.len().max(1) as u32,
        outer_oz * 0.035,
        &spec.mask_color,
    )
}

fn report(board: &BoardGeometry, spec: &Spec) -> Result<Vec<Line>> {
    let floors = floors(board, spec)?;
    let track_min = floors.track_space_mm;
    let pad_track_min = floors.smd_pad_to_track_mm.unwrap_or(track_min);
    let mask_bridge = floors.mask_bridge_mm.unwrap_or(0.10);
    let mask_trace = floors.mask_to_copper_mm.unwrap_or(0.09);
    let annular_min = floors.annular_fail_mm.unwrap_or(0.15);
    let annular_rec = floors.annular_warn_mm;

    let hits = copper_hits(board);
    let mut lines = Vec::new();
    lines.push(clearance_line(
        "different-net-spacing",
        &hits,
        |hit| !hit.same_net && hit.distance >= CONTACT,
        |hit| hit.distance,
        Band {
            fail_below: Some(track_min),
            warn_below: None,
        },
        "different-net copper",
    ));
    lines.push(clearance_line(
        "same-net-spacing",
        &hits,
        |hit| hit.same_net && hit.distance >= CONTACT,
        |hit| hit.distance,
        Band {
            fail_below: Some(track_min),
            warn_below: Some(0.25),
        },
        "same-net copper",
    ));
    lines.push(clearance_line(
        "smd-pad-to-track",
        &hits,
        |hit| !hit.same_net && hit.smd_to_track && hit.distance >= CONTACT,
        |hit| hit.distance,
        Band {
            fail_below: Some(pad_track_min),
            warn_below: None,
        },
        "SMD pad to track",
    ));
    lines.push(clearance_line(
        "smd-pad-spacing",
        &hits,
        |hit| !hit.same_net && hit.smd_to_smd && hit.distance >= CONTACT,
        |hit| hit.distance,
        Band {
            fail_below: Some(floors.smd_pad_spacing_mm.unwrap_or(0.15)),
            warn_below: None,
        },
        "SMD pad to SMD pad",
    ));
    lines.push(clearance_line(
        "mask-bridge",
        &hits,
        |hit| !hit.same_net && hit.smd_to_smd && hit.mask_gap >= CONTACT,
        |hit| hit.mask_gap,
        Band {
            fail_below: Some(mask_bridge),
            warn_below: None,
        },
        "solder-mask bridge",
    ));
    lines.push(clearance_line(
        "mask-to-trace",
        &hits,
        |hit| !hit.same_net && hit.smd_to_track && hit.mask_gap >= CONTACT,
        |hit| hit.mask_gap,
        Band {
            fail_below: Some(mask_trace),
            warn_below: None,
        },
        "solder-mask opening to trace",
    ));

    lines.push(annular_line(board, annular_min, annular_rec));
    lines.push(via_diameter_line(
        board,
        floors.via_extra_fail_mm.unwrap_or(0.10),
        floors.via_extra_warn_mm.or(Some(0.15)),
    ));
    lines.push(hole_size_line(
        board,
        floors.pth_drill_fail_mm.unwrap_or(0.15),
        floors.via_drill_warn_mm,
    ));
    lines.extend(hole_copper_lines(board, &floors));
    lines.push(copper_edge_line(
        board,
        floors.copper_to_edge_mm.unwrap_or(0.20),
    ));
    lines.push(npth_diameter_line(
        board,
        floors.npth_drill_fail_mm.unwrap_or(0.50),
    ));
    lines.push(npth_annular_line(
        board,
        floors.npth_annular_warn_mm.unwrap_or(0.45),
    ));
    lines.push(slot_line(
        board,
        true,
        floors.plated_slot_mm.unwrap_or(0.50),
    ));
    lines.push(slot_line(board, false, floors.npth_slot_mm.unwrap_or(1.0)));
    lines.push(hole_spacing_line(
        board,
        true,
        floors.via_hole_spacing_mm.unwrap_or(0.20),
    ));
    lines.push(hole_spacing_line(
        board,
        false,
        floors.pth_hole_spacing_mm.unwrap_or(0.45),
    ));
    lines.push(silk_width_line(
        board,
        floors.silk_stroke_mm.unwrap_or(0.15),
    ));
    lines.push(silk_height_line(
        board,
        floors.silk_height_mm.unwrap_or(1.0),
    ));
    lines.push(silk_pad_line(board, floors.silk_to_pad_mm.unwrap_or(0.15)));
    lines.push(pad_size_line(board));
    lines.push(hole_in_pad_line(board));
    lines.push(body_edge_line(board, spec));
    lines.push(body_spacing_line(board));
    Ok(lines)
}

struct Hit {
    distance: f64,
    label: String,
    same_net: bool,
    smd_to_smd: bool,
    smd_to_track: bool,
    /// Copper gap minus the solder-mask expansion of each pad. Tracks expand by 0.
    mask_gap: f64,
}

fn copper_hits(board: &BoardGeometry) -> Vec<Hit> {
    let items: Vec<&Shape> = board
        .shapes
        .iter()
        .filter(|shape| {
            matches!(
                shape.source,
                Source::Track | Source::Pad | Source::Via | Source::Zone
            ) && shape.layer.ends_with(".Cu")
        })
        .collect();
    let mut hits = Vec::new();
    let mut grid: HashMap<(i32, i32), Vec<usize>> = HashMap::new();
    let mut large = Vec::new();
    for (index, shape) in items.iter().enumerate() {
        let window = outline_window(&shape.occupied);
        let span = (window.x_max - window.x_min).max(window.y_max - window.y_min);
        if shape.source == Source::Zone && span > 8.0 {
            large.push(index);
            continue;
        }
        for cell in cells(window, 0.0) {
            grid.entry(cell).or_default().push(index);
        }
    }
    let mut seen = HashMap::<(usize, usize), ()>::new();
    for (index, shape) in items.iter().enumerate() {
        if large.contains(&index) {
            continue;
        }
        let window = outline_window(&shape.occupied);
        let mut near = Vec::new();
        for cell in cells(window, SEARCH) {
            if let Some(list) = grid.get(&cell) {
                near.extend(list.iter().copied());
            }
        }
        near.sort_unstable();
        near.dedup();
        for other in near {
            if other <= index {
                continue;
            }
            if seen.insert((index, other), ()).is_some() {
                continue;
            }
            consider(&items, index, other, &mut hits);
        }
    }
    for &zone in &large {
        let segments = boundary_segments(&items[zone].occupied);
        let mut zone_grid: HashMap<(i32, i32), Vec<(Point, Point)>> = HashMap::new();
        for (start, end) in &segments {
            let window = Window {
                x_min: start.x.min(end.x),
                x_max: start.x.max(end.x),
                y_min: start.y.min(end.y),
                y_max: start.y.max(end.y),
            };
            for cell in cells(window, 0.0) {
                zone_grid.entry(cell).or_default().push((*start, *end));
            }
        }
        for (index, shape) in items.iter().enumerate() {
            if index == zone || large.contains(&index) {
                continue;
            }
            if shape.layer != items[zone].layer {
                continue;
            }
            if same_pad(shape, items[zone]) {
                continue;
            }
            let mut best = f64::MAX;
            let mut at = interior(&shape.occupied);
            for (start, end) in boundary_segments(&shape.occupied) {
                let query = Window {
                    x_min: start.x.min(end.x),
                    x_max: start.x.max(end.x),
                    y_min: start.y.min(end.y),
                    y_max: start.y.max(end.y),
                };
                for cell in cells(query, SEARCH) {
                    if let Some(edges) = zone_grid.get(&cell) {
                        for (other_start, other_end) in edges {
                            let (gap, mid) = segment_gap(start, end, *other_start, *other_end);
                            if gap < best {
                                best = gap;
                                at = mid;
                            }
                        }
                    }
                }
            }
            let inside = contains(&items[zone].occupied, interior(&shape.occupied));
            let distance = if inside && best > CONTACT { 0.0 } else { best };
            if distance > SEARCH {
                continue;
            }
            push_hit(&items, index, zone, distance, at, &mut hits);
        }
    }
    hits
}

fn consider(items: &[&Shape], left: usize, right: usize, hits: &mut Vec<Hit>) {
    let a = items[left];
    let b = items[right];
    if a.layer != b.layer || same_pad(a, b) {
        return;
    }
    let aw = outline_window(&a.occupied);
    let bw = outline_window(&b.occupied);
    if window_gap(aw, bw) > SEARCH {
        return;
    }
    let gap = filled_separation(&a.occupied, &b.occupied);
    if gap.distance > SEARCH {
        return;
    }
    push_hit(items, left, right, gap.distance, gap.at, hits);
}

fn push_hit(
    items: &[&Shape],
    left: usize,
    right: usize,
    distance: f64,
    at: Point,
    hits: &mut Vec<Hit>,
) {
    let a = items[left];
    let b = items[right];
    let same_net = a.net.is_some() && a.net == b.net;
    if same_net && distance < CONTACT {
        return;
    }
    let smd = |shape: &Shape| shape.pad_form == Some(PadForm::Smd) && shape.source == Source::Pad;
    let trackish =
        |shape: &Shape| matches!(shape.source, Source::Track | Source::Via | Source::Zone);
    let expansion = |shape: &Shape| {
        if shape.pad_form == Some(PadForm::Smd) {
            shape.mask_expansion_mm
        } else {
            0.0
        }
    };
    hits.push(Hit {
        distance,
        label: format!(
            "{} {} {:.3} mm at {:.2},{:.2}",
            pair_name(a, b),
            a.layer,
            distance,
            at.x,
            at.y
        ),
        same_net,
        smd_to_smd: smd(a) && smd(b),
        smd_to_track: (smd(a) && trackish(b)) || (smd(b) && trackish(a)),
        mask_gap: (distance - expansion(a) - expansion(b)).max(0.0),
    });
}

fn pair_name(a: &Shape, b: &Shape) -> String {
    format!("{}–{}", shape_name(a), shape_name(b))
}

fn shape_name(shape: &Shape) -> String {
    match (&shape.reference, &shape.pad) {
        (Some(reference), Some(pad)) if !pad.is_empty() => format!("{reference}.{pad}"),
        (Some(reference), _) => reference.clone(),
        _ => shape.source.as_str().to_string(),
    }
}

fn same_pad(a: &Shape, b: &Shape) -> bool {
    matches!(
        (&a.reference, &a.pad, &b.reference, &b.pad),
        (Some(ar), Some(ap), Some(br), Some(bp)) if ar == br && ap == bp && !ap.is_empty()
    )
}

fn clearance_line(
    name: &str,
    hits: &[Hit],
    keep: impl Fn(&Hit) -> bool,
    distance_of: impl Fn(&Hit) -> f64,
    band: Band,
    what: &str,
) -> Line {
    let mut sites: Vec<Site> = hits
        .iter()
        .filter(|hit| keep(hit))
        .filter(|hit| worse_than(distance_of(hit), band))
        .map(|hit| Site {
            distance: distance_of(hit),
            label: hit.label.clone(),
        })
        .collect();
    sites.sort_by(|left, right| left.distance.partial_cmp(&right.distance).unwrap());
    line_from_sites(name, sites, band, what)
}

fn annular_line(board: &BoardGeometry, minimum: f64, recommended: Option<f64>) -> Line {
    let mut sites = Vec::new();
    for hole in holes(board) {
        if hole.shape.pad_form == Some(PadForm::NonPlated) {
            continue;
        }
        let Some(ring) = annular(board, &hole) else {
            continue;
        };
        if !worse_than(
            ring,
            Band {
                fail_below: Some(minimum),
                warn_below: recommended,
            },
        ) {
            continue;
        }
        sites.push(Site {
            distance: ring,
            label: format!(
                "{} {:.3} mm annular at {:.2},{:.2}",
                hole_name(&hole),
                ring,
                hole.center.x,
                hole.center.y
            ),
        });
    }
    line_from_sites(
        "annular-ring",
        sites,
        Band {
            fail_below: Some(minimum),
            warn_below: recommended,
        },
        "annular ring",
    )
}

fn via_diameter_line(board: &BoardGeometry, fail: f64, warn: Option<f64>) -> Line {
    let mut sites = Vec::new();
    let skip = warn.unwrap_or(fail);
    for hole in holes(board).filter(|hole| hole.via) {
        let Some(pad) = via_pad(board, &hole) else {
            continue;
        };
        let extra = pad - hole.drill;
        if extra + 1e-6 >= skip {
            continue;
        }
        sites.push(Site {
            distance: extra,
            label: format!(
                "via diameter {:.3} mm is {:.3} mm above the {:.3} mm hole at {:.2},{:.2}",
                pad, extra, hole.drill, hole.center.x, hole.center.y
            ),
        });
    }
    line_from_sites(
        "via-diameter",
        sites,
        Band {
            fail_below: Some(fail),
            warn_below: warn,
        },
        "via diameter above the hole",
    )
}

fn hole_size_line(board: &BoardGeometry, fail_at: f64, via_warn: Option<f64>) -> Line {
    let mut sites = Vec::new();
    for hole in holes(board) {
        if hole.shape.pad_form == Some(PadForm::NonPlated) {
            continue;
        }
        let warn_at = if hole.via { via_warn } else { None };
        if !worse_than(
            hole.drill,
            Band {
                fail_below: Some(fail_at),
                warn_below: warn_at,
            },
        ) {
            continue;
        }
        sites.push(Site {
            distance: hole.drill,
            label: format!(
                "{} drill {:.3} mm at {:.2},{:.2}",
                hole_name(&hole),
                hole.drill,
                hole.center.x,
                hole.center.y
            ),
        });
    }
    line_from_sites(
        "plated-hole",
        sites,
        Band {
            fail_below: Some(fail_at),
            warn_below: via_warn,
        },
        "plated hole",
    )
}

fn hole_copper_lines(board: &BoardGeometry, floors: &Limits) -> Vec<Line> {
    let rules = [
        (
            "via-hole-to-copper",
            true,
            false,
            true,
            floors.via_hole_copper_mm.unwrap_or(0.20),
            None,
        ),
        (
            "pth-to-copper",
            false,
            true,
            true,
            floors.pth_hole_copper_outer_fail_mm.unwrap_or(0.28),
            floors.pth_hole_copper_outer_warn_mm,
        ),
        (
            "npth-to-copper",
            false,
            false,
            true,
            floors.npth_hole_copper_mm.unwrap_or(0.20),
            None,
        ),
        (
            "inner-via-hole-to-copper",
            true,
            false,
            false,
            floors.via_hole_copper_mm.unwrap_or(0.20),
            None,
        ),
        (
            "inner-pth-to-copper",
            false,
            true,
            false,
            floors.pth_hole_copper_inner_mm.unwrap_or(0.30),
            None,
        ),
    ];
    rules
        .into_iter()
        .map(|(name, via, pth, outer, minimum, recommended)| {
            let mut sites = Vec::new();
            for hole in holes(board) {
                let kind_ok = if via {
                    hole.via
                } else if pth {
                    hole.shape.pad_form == Some(PadForm::Through)
                } else {
                    hole.shape.pad_form == Some(PadForm::NonPlated)
                };
                if !kind_ok {
                    continue;
                }
                for shape in &board.shapes {
                    if !shape.layer.ends_with(".Cu") {
                        continue;
                    }
                    let is_outer = shape.layer == "F.Cu" || shape.layer == "B.Cu";
                    if is_outer != outer {
                        continue;
                    }
                    if owns_hole(&hole, shape) {
                        continue;
                    }
                    if window_gap(
                        outline_window(&hole.shape.occupied),
                        outline_window(&shape.occupied),
                    ) > SEARCH
                    {
                        continue;
                    }
                    let gap = filled_separation(&hole.shape.occupied, &shape.occupied);
                    let same = hole.shape.net.is_some() && hole.shape.net == shape.net;
                    if same && gap.distance < CONTACT {
                        continue;
                    }
                    if !worse_than(
                        gap.distance,
                        Band {
                            fail_below: Some(minimum),
                            warn_below: recommended,
                        },
                    ) {
                        continue;
                    }
                    sites.push(Site {
                        distance: gap.distance,
                        label: format!(
                            "{} to {} {:.3} mm on {}",
                            hole_name(&hole),
                            shape_name(shape),
                            gap.distance,
                            shape.layer
                        ),
                    });
                }
            }
            line_from_sites(
                name,
                sites,
                Band {
                    fail_below: Some(minimum),
                    warn_below: recommended,
                },
                name,
            )
        })
        .collect()
}

fn copper_edge_line(board: &BoardGeometry, minimum: f64) -> Line {
    let edges: Vec<&Shape> = board
        .shapes
        .iter()
        .filter(|shape| shape.source == Source::Edge && shape.inset)
        .collect();
    let mut sites = Vec::new();
    if let Some(outer) = edges.iter().max_by(|left, right| {
        let left = outline_window(&left.occupied);
        let right = outline_window(&right.occupied);
        span(left).partial_cmp(&span(right)).unwrap()
    }) {
        for shape in &board.shapes {
            if !shape.layer.ends_with(".Cu") {
                continue;
            }
            let point = interior(&shape.occupied);
            if !contains(&outer.occupied, point) {
                sites.push(Site {
                    distance: 0.0,
                    label: format!(
                        "{} crosses the board edge at {:.2},{:.2}",
                        shape_name(shape),
                        point.x,
                        point.y
                    ),
                });
                continue;
            }
            for edge in &edges {
                let gap = boundary_gap(&shape.occupied, &edge.occupied);
                if gap.distance + 1e-6 < minimum {
                    sites.push(Site {
                        distance: gap.distance,
                        label: format!(
                            "{} {:.3} mm from the edge at {:.2},{:.2}",
                            shape_name(shape),
                            gap.distance,
                            gap.at.x,
                            gap.at.y
                        ),
                    });
                }
            }
        }
    }
    line_from_sites(
        "copper-to-edge",
        sites,
        Band {
            fail_below: Some(minimum),
            warn_below: None,
        },
        "copper to board edge",
    )
}

fn npth_diameter_line(board: &BoardGeometry, minimum: f64) -> Line {
    let mut sites = Vec::new();
    for hole in holes(board).filter(|hole| hole.shape.pad_form == Some(PadForm::NonPlated)) {
        if hole.drill + 1e-6 >= minimum {
            continue;
        }
        sites.push(Site {
            distance: hole.drill,
            label: format!(
                "{} NPTH {:.3} mm at {:.2},{:.2}",
                hole_name(&hole),
                hole.drill,
                hole.center.x,
                hole.center.y
            ),
        });
    }
    line_from_sites(
        "npth-diameter",
        sites,
        Band {
            fail_below: Some(minimum),
            warn_below: None,
        },
        "NPTH diameter",
    )
}

fn npth_annular_line(board: &BoardGeometry, minimum: f64) -> Line {
    let mut sites = Vec::new();
    for hole in holes(board).filter(|hole| hole.shape.pad_form == Some(PadForm::NonPlated)) {
        let Some(ring) = annular(board, &hole) else {
            continue;
        };
        if ring + 1e-6 >= minimum {
            continue;
        }
        sites.push(Site {
            distance: ring,
            label: format!(
                "{} NPTH annular {:.3} mm at {:.2},{:.2}",
                hole_name(&hole),
                ring,
                hole.center.x,
                hole.center.y
            ),
        });
    }
    line_from_sites(
        "npth-annular",
        sites,
        Band {
            fail_below: None,
            warn_below: Some(minimum),
        },
        "NPTH annular ring",
    )
}

fn slot_line(board: &BoardGeometry, plated: bool, minimum: f64) -> Line {
    let mut sites = Vec::new();
    for hole in holes(board) {
        let is_plated = hole.shape.pad_form == Some(PadForm::Through);
        if is_plated != plated {
            continue;
        }
        let Some(length) = hole.shape.drill_length_mm else {
            continue;
        };
        if length <= hole.drill + 0.01 {
            continue;
        }
        if hole.drill + 1e-6 < minimum {
            sites.push(Site {
                distance: hole.drill,
                label: format!(
                    "{} slot width {:.3} mm at {:.2},{:.2}",
                    hole_name(&hole),
                    hole.drill,
                    hole.center.x,
                    hole.center.y
                ),
            });
        }
        if length + 1e-6 < hole.drill * 2.0 {
            sites.push(Site {
                distance: length / hole.drill,
                label: format!(
                    "{} slot length {:.3} mm is under twice its {:.3} mm width",
                    hole_name(&hole),
                    length,
                    hole.drill
                ),
            });
        }
    }
    line_from_sites(
        if plated { "plated-slot" } else { "npth-slot" },
        sites,
        Band {
            fail_below: Some(minimum),
            warn_below: None,
        },
        if plated {
            "plated slot"
        } else {
            "non-plated slot"
        },
    )
}

fn hole_spacing_line(board: &BoardGeometry, via: bool, minimum: f64) -> Line {
    let selected: Vec<Hole<'_>> = holes(board)
        .filter(|hole| {
            if via {
                hole.via
            } else {
                hole.shape.pad_form == Some(PadForm::Through)
            }
        })
        .collect();
    let mut sites = Vec::new();
    for (index, hole) in selected.iter().enumerate() {
        for other in selected.iter().skip(index + 1) {
            let gap = filled_separation(&hole.shape.occupied, &other.shape.occupied);
            if gap.distance + 1e-6 >= minimum || gap.distance > SEARCH {
                continue;
            }
            sites.push(Site {
                distance: gap.distance,
                label: format!(
                    "{} to {} {:.3} mm",
                    hole_name(&hole),
                    hole_name(other),
                    gap.distance
                ),
            });
        }
    }
    line_from_sites(
        if via {
            "via-hole-spacing"
        } else {
            "pth-hole-spacing"
        },
        sites,
        Band {
            fail_below: Some(minimum),
            warn_below: None,
        },
        if via {
            "via hole spacing"
        } else {
            "PTH hole spacing"
        },
    )
}

fn silk_width_line(board: &BoardGeometry, minimum: f64) -> Line {
    let mut sites = Vec::new();
    for shape in &board.shapes {
        if !shape.layer.ends_with("SilkS") {
            continue;
        }
        let Some(width) = shape.width else {
            continue;
        };
        if width <= 0.0 || width + 1e-6 >= minimum {
            continue;
        }
        let at = interior(&shape.occupied);
        sites.push(Site {
            distance: width,
            label: format!(
                "{} silk {:.3} mm on {} at {:.2},{:.2}",
                shape_name(shape),
                width,
                shape.layer,
                at.x,
                at.y
            ),
        });
    }
    line_from_sites(
        "silk-width",
        sites,
        Band {
            fail_below: None,
            warn_below: Some(minimum),
        },
        "silkscreen stroke",
    )
}

fn silk_height_line(board: &BoardGeometry, minimum: f64) -> Line {
    let mut sites = Vec::new();
    for shape in &board.shapes {
        let Some(height) = shape.text_height_mm else {
            continue;
        };
        if height + 1e-6 >= minimum {
            continue;
        }
        let at = interior(&shape.occupied);
        sites.push(Site {
            distance: height,
            label: format!(
                "{} silk text {:.3} mm tall at {:.2},{:.2}",
                shape_name(shape),
                height,
                at.x,
                at.y
            ),
        });
    }
    line_from_sites(
        "silk-height",
        sites,
        Band {
            fail_below: None,
            warn_below: Some(minimum),
        },
        "silkscreen text height",
    )
}

fn silk_pad_line(board: &BoardGeometry, minimum: f64) -> Line {
    let pads: Vec<&Shape> = board
        .shapes
        .iter()
        .filter(|shape| shape.source == Source::Pad && shape.layer.ends_with(".Cu"))
        .collect();
    let mut sites = Vec::new();
    for silk in board
        .shapes
        .iter()
        .filter(|shape| shape.layer.ends_with("SilkS"))
    {
        let side = if silk.layer.starts_with('B') {
            "B"
        } else {
            "F"
        };
        for pad in &pads {
            if !pad.layer.starts_with(side) {
                continue;
            }
            if window_gap(
                outline_window(&silk.occupied),
                outline_window(&pad.occupied),
            ) > minimum
            {
                continue;
            }
            let gap = filled_separation(&silk.occupied, &pad.occupied);
            if gap.distance + 1e-6 >= minimum {
                continue;
            }
            sites.push(Site {
                distance: gap.distance,
                label: format!(
                    "{} silk {:.3} mm from {} at {:.2},{:.2}",
                    shape_name(silk),
                    gap.distance,
                    shape_name(pad),
                    gap.at.x,
                    gap.at.y
                ),
            });
        }
    }
    line_from_sites(
        "silk-to-pad",
        sites,
        Band {
            fail_below: None,
            warn_below: Some(minimum),
        },
        "silkscreen to pad",
    )
}

fn pad_size_line(board: &BoardGeometry) -> Line {
    let mut sites = Vec::new();
    let mut seen = HashMap::<(String, String), ()>::new();
    for shape in &board.shapes {
        if shape.pad_form != Some(PadForm::Smd) || shape.source != Source::Pad {
            continue;
        }
        let key = (
            shape.reference.clone().unwrap_or_default(),
            shape.pad.clone().unwrap_or_default(),
        );
        if seen.insert(key, ()).is_some() {
            continue;
        }
        let window = outline_window(&shape.occupied);
        let width = window.x_max - window.x_min;
        let height = window.y_max - window.y_min;
        let small = width.min(height);
        if small + 1e-6 >= 0.25 {
            continue;
        }
        sites.push(Site {
            distance: small,
            label: format!("{} {:.3}×{:.3} mm", shape_name(shape), width, height),
        });
    }
    line_from_sites(
        "smd-pad-size",
        sites,
        Band {
            fail_below: None,
            warn_below: Some(0.25),
        },
        "SMD pad",
    )
}

fn hole_in_pad_line(board: &BoardGeometry) -> Line {
    let pads: Vec<&Shape> = board
        .shapes
        .iter()
        .filter(|shape| shape.pad_form == Some(PadForm::Smd) && shape.source == Source::Pad)
        .collect();
    let mut sites = Vec::new();
    for hole in holes(board) {
        for pad in &pads {
            if !contains(&pad.occupied, hole.center) {
                continue;
            }
            sites.push(Site {
                distance: 0.0,
                label: format!(
                    "{} hole inside {} at {:.2},{:.2}",
                    hole_name(&hole),
                    shape_name(pad),
                    hole.center.x,
                    hole.center.y
                ),
            });
            break;
        }
    }
    line_from_sites(
        "hole-in-smd-pad",
        sites,
        Band {
            fail_below: None,
            warn_below: Some(0.001),
        },
        "hole in an SMD pad",
    )
}

fn body_edge_line(board: &BoardGeometry, spec: &Spec) -> Line {
    let Some(board_box) = board_box(board) else {
        return pass("body-to-edge");
    };
    let allowed: Vec<&str> = spec.allow_overhang.iter().map(String::as_str).collect();
    let mut sites = Vec::new();
    let mut worst = TestStatus::Passed;
    for body in bodies(board) {
        if allowed.contains(&body.reference.as_str()) {
            continue;
        }
        let gaps = [
            ("left", body.bounds.x_min - board_box.x_min),
            ("right", board_box.x_max - body.bounds.x_max),
            ("bottom", body.bounds.y_min - board_box.y_min),
            ("top", board_box.y_max - body.bounds.y_max),
        ];
        for (side, gap) in gaps {
            if gap + 1e-6 >= 2.5 {
                continue;
            }
            let railed = spec.rails.iter().any(|rail| rail == side);
            let status = if railed {
                TestStatus::Warning
            } else {
                TestStatus::Failed
            };
            if status == TestStatus::Failed {
                worst = TestStatus::Failed;
            } else if worst == TestStatus::Passed {
                worst = TestStatus::Warning;
            }
            sites.push(Site {
                distance: gap.max(0.0),
                label: format!(
                    "{} {:.3} mm from the {side} edge{}",
                    body.reference,
                    gap,
                    if railed { " (rail on that edge)" } else { "" }
                ),
            });
        }
    }
    sites.sort_by(|left, right| left.distance.partial_cmp(&right.distance).unwrap());
    Line {
        name: "body-to-edge".into(),
        subtest: true,
        status: if sites.is_empty() {
            TestStatus::Passed
        } else {
            worst
        },
        details: site_labels(&sites),
        assertions: 1 + sites.len(),
    }
}

fn body_spacing_line(board: &BoardGeometry) -> Line {
    let bodies = bodies(board);
    let mut sites = Vec::new();
    let mut failed = false;
    for (index, body) in bodies.iter().enumerate() {
        for other in bodies.iter().skip(index + 1) {
            if window_gap(body.bounds, other.bounds) > 2.0 {
                continue;
            }
            let mut best = f64::MAX;
            for left in &body.outlines {
                for right in &other.outlines {
                    let gap = filled_separation(left, right);
                    if gap.distance < best {
                        best = gap.distance;
                    }
                }
            }
            if best < CONTACT {
                failed = true;
                sites.push(Site {
                    distance: 0.0,
                    label: format!("{} overlaps {}", body.reference, other.reference),
                });
                continue;
            }
            let Some(limit) = package_gap(body.package, other.package) else {
                continue;
            };
            if best + 1e-6 >= limit {
                continue;
            }
            sites.push(Site {
                distance: best,
                label: format!(
                    "{} to {} {:.3} mm, recommended {:.2} mm",
                    body.reference, other.reference, best, limit
                ),
            });
        }
    }
    sites.sort_by(|left, right| left.distance.partial_cmp(&right.distance).unwrap());
    let status = if failed {
        TestStatus::Failed
    } else if sites.is_empty() {
        TestStatus::Passed
    } else {
        TestStatus::Warning
    };
    Line {
        name: "body-spacing".into(),
        subtest: true,
        status,
        details: site_labels(&sites),
        assertions: 1 + sites.len(),
    }
}

struct Body {
    reference: String,
    package: Option<Package>,
    outlines: Vec<Outline>,
    bounds: Window,
}

fn bodies(board: &BoardGeometry) -> Vec<Body> {
    let mut groups: HashMap<String, Vec<&Shape>> = HashMap::new();
    for shape in &board.shapes {
        let Some(reference) = shape.reference.clone() else {
            continue;
        };
        if shape.layer.ends_with(".Fab") || shape.layer.contains("CrtYd") {
            groups.entry(reference).or_default().push(shape);
        }
    }
    let names = footprint_names(&board.footprints);
    groups
        .into_iter()
        .filter_map(|(reference, shapes)| {
            let fab: Vec<&Shape> = shapes
                .iter()
                .copied()
                .filter(|shape| shape.layer.ends_with(".Fab"))
                .collect();
            let chosen = if fab.is_empty() { shapes } else { fab };
            if chosen.is_empty() {
                return None;
            }
            let outlines: Vec<Outline> =
                chosen.iter().map(|shape| shape.occupied.clone()).collect();
            let mut bounds = outline_window(&outlines[0]);
            for outline in outlines.iter().skip(1) {
                let next = outline_window(outline);
                bounds.x_min = bounds.x_min.min(next.x_min);
                bounds.y_min = bounds.y_min.min(next.y_min);
                bounds.x_max = bounds.x_max.max(next.x_max);
                bounds.y_max = bounds.y_max.max(next.y_max);
            }
            Some(Body {
                package: names.get(&reference).and_then(|name| package(name)),
                reference,
                outlines,
                bounds,
            })
        })
        .collect()
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Package {
    C0201,
    C0402,
    C0603,
    C0805,
    C1206,
    Qfn,
    Qfp,
    Sop,
    Sot,
    Bga,
}

fn package(name: &str) -> Option<Package> {
    let name = name.to_ascii_lowercase();
    if name.contains("0201") {
        Some(Package::C0201)
    } else if name.contains("0402") {
        Some(Package::C0402)
    } else if name.contains("0603") {
        Some(Package::C0603)
    } else if name.contains("0805") {
        Some(Package::C0805)
    } else if name.contains("1206") {
        Some(Package::C1206)
    } else if name.contains("bga") {
        Some(Package::Bga)
    } else if name.contains("qfp") {
        Some(Package::Qfp)
    } else if name.contains("qfn") || name.contains("xdfn") || name.contains("dfn") {
        Some(Package::Qfn)
    } else if name.contains("soic") || name.contains("sop") || name.contains("tssop") {
        Some(Package::Sop)
    } else if name.contains("sot") {
        Some(Package::Sot)
    } else {
        None
    }
}

fn package_gap(left: Option<Package>, right: Option<Package>) -> Option<f64> {
    let (left, right) = (left?, right?);
    let table = [
        [0.15, 0.15, 0.18, 0.18, 0.25, 1.0, 0.5, 0.4, 0.2, 1.0],
        [0.15, 0.15, 0.18, 0.18, 0.25, 1.0, 0.5, 0.4, 0.2, 1.0],
        [0.18, 0.18, 0.18, 0.18, 0.25, 1.0, 0.5, 0.4, 0.2, 1.0],
        [0.18, 0.18, 0.18, 0.18, 0.25, 1.0, 0.5, 0.4, 0.2, 1.0],
        [0.25, 0.25, 0.25, 0.25, 0.35, 1.0, 0.5, 0.4, 0.2, 1.0],
        [1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.5],
        [0.5, 0.5, 0.5, 0.5, 0.5, 1.0, 1.25, 1.25, 1.0, 1.5],
        [0.4, 0.4, 0.4, 0.4, 0.4, 1.0, 1.25, 0.5, 0.4, 1.0],
        [0.2, 0.2, 0.2, 0.2, 0.2, 1.0, 1.0, 0.4, 0.4, 1.0],
        [1.0, 1.0, 1.0, 1.0, 1.0, 1.5, 1.5, 1.0, 1.0, 2.0],
    ];
    Some(table[left as usize][right as usize])
}

struct Hole<'a> {
    shape: &'a Shape,
    center: Point,
    drill: f64,
    via: bool,
}

fn holes(board: &BoardGeometry) -> impl Iterator<Item = Hole<'_>> {
    board.shapes.iter().filter_map(|shape| {
        if shape.source != Source::Hole {
            return None;
        }
        let window = outline_window(&shape.occupied);
        let drill = shape
            .width
            .unwrap_or((window.x_max - window.x_min).min(window.y_max - window.y_min));
        Some(Hole {
            shape,
            center: Point::new(
                (window.x_min + window.x_max) / 2.0,
                (window.y_min + window.y_max) / 2.0,
            ),
            drill,
            via: shape.reference.is_none(),
        })
    })
}

fn hole_name(hole: &Hole<'_>) -> String {
    shape_name(hole.shape)
}

fn annular(board: &BoardGeometry, hole: &Hole<'_>) -> Option<f64> {
    let mut best: Option<f64> = None;
    for shape in &board.shapes {
        if !owns_hole(&hole, shape) || !shape.layer.ends_with(".Cu") {
            continue;
        }
        let reach = match &shape.occupied {
            Outline::Circle { center, radius } => {
                distance_points(*center, hole.center).max(*radius)
            }
            _ => {
                boundary_gap(
                    &Outline::Circle {
                        center: hole.center,
                        radius: 0.0,
                    },
                    &shape.occupied,
                )
                .distance
            }
        };
        let ring = reach - hole.drill / 2.0;
        best = Some(best.map(|current| current.min(ring)).unwrap_or(ring));
    }
    best
}

fn via_pad(board: &BoardGeometry, hole: &Hole<'_>) -> Option<f64> {
    board.shapes.iter().find_map(|shape| {
        if shape.source != Source::Via {
            return None;
        }
        let Outline::Circle { center, radius } = shape.occupied else {
            return None;
        };
        if distance_points(center, hole.center) > 0.01 {
            return None;
        }
        Some(radius * 2.0)
    })
}

fn owns_hole(hole: &Hole<'_>, shape: &Shape) -> bool {
    if shape.source == Source::Via && hole.via {
        let Outline::Circle { center, .. } = shape.occupied else {
            return false;
        };
        return distance_points(center, hole.center) <= 0.01;
    }
    shape.source == Source::Pad && same_pad(hole.shape, shape)
}

fn board_box(board: &BoardGeometry) -> Option<Window> {
    let mut bounds: Option<Window> = None;
    for shape in &board.shapes {
        if shape.source != Source::Edge {
            continue;
        }
        let next = outline_window(&shape.occupied);
        bounds = Some(match bounds {
            None => next,
            Some(so_far) => Window {
                x_min: so_far.x_min.min(next.x_min),
                y_min: so_far.y_min.min(next.y_min),
                x_max: so_far.x_max.max(next.x_max),
                y_max: so_far.y_max.max(next.y_max),
            },
        });
    }
    bounds
}

fn line_from_sites(name: &str, mut sites: Vec<Site>, band: Band, what: &str) -> Line {
    sites.sort_by(|left, right| left.distance.partial_cmp(&right.distance).unwrap());
    let status = sites.iter().fold(TestStatus::Passed, |status, site| {
        let next = status_for(site.distance, band);
        worse(status, next)
    });
    let mut details = Vec::new();
    if let Some(limit) = band.fail_below {
        if matches!(status, TestStatus::Failed) {
            details.push(format!("{what} minimum is {limit:.3} mm"));
        }
    }
    if let Some(limit) = band.warn_below {
        if matches!(status, TestStatus::Warning) {
            details.push(format!("{what} recommendation is {limit:.3} mm"));
        }
    }
    details.extend(site_labels(&sites));
    Line {
        name: name.into(),
        subtest: true,
        status,
        details,
        assertions: 1 + sites.len(),
    }
}

fn pass(name: &str) -> Line {
    Line {
        name: name.into(),
        subtest: true,
        status: TestStatus::Passed,
        details: Vec::new(),
        assertions: 1,
    }
}

fn status_for(distance: f64, band: Band) -> TestStatus {
    if band.fail_below.is_some_and(|limit| distance + 1e-6 < limit) {
        TestStatus::Failed
    } else if band.warn_below.is_some_and(|limit| distance + 1e-6 < limit) {
        TestStatus::Warning
    } else {
        TestStatus::Passed
    }
}

fn worse_than(distance: f64, band: Band) -> bool {
    status_for(distance, band) != TestStatus::Passed
}

fn worse(left: TestStatus, right: TestStatus) -> TestStatus {
    match (left, right) {
        (TestStatus::Failed, _) | (_, TestStatus::Failed) => TestStatus::Failed,
        (TestStatus::Warning, _) | (_, TestStatus::Warning) => TestStatus::Warning,
        _ => TestStatus::Passed,
    }
}

fn site_labels(sites: &[Site]) -> Vec<String> {
    sites.iter().map(|site| site.label.clone()).collect()
}

fn ounces(thickness_mm: Option<f64>, fallback: f64) -> f64 {
    match thickness_mm {
        Some(mm) if mm >= 0.06 => 2.0,
        Some(mm) if mm >= 0.025 => 1.0,
        Some(_) => 0.5,
        None => fallback,
    }
}

fn cells(window: Window, extra: f64) -> Vec<(i32, i32)> {
    let mut out = Vec::new();
    let x0 = ((window.x_min - extra) / 1.0).floor() as i32;
    let x1 = ((window.x_max + extra) / 1.0).floor() as i32;
    let y0 = ((window.y_min - extra) / 1.0).floor() as i32;
    let y1 = ((window.y_max + extra) / 1.0).floor() as i32;
    for x in x0..=x1 {
        for y in y0..=y1 {
            out.push((x, y));
        }
    }
    out
}

fn span(window: Window) -> f64 {
    (window.x_max - window.x_min).hypot(window.y_max - window.y_min)
}

fn window_gap(a: Window, b: Window) -> f64 {
    let dx = if a.x_max < b.x_min {
        b.x_min - a.x_max
    } else if b.x_max < a.x_min {
        a.x_min - b.x_max
    } else {
        0.0
    };
    let dy = if a.y_max < b.y_min {
        b.y_min - a.y_max
    } else if b.y_max < a.y_min {
        a.y_min - b.y_max
    } else {
        0.0
    };
    (dx * dx + dy * dy).sqrt()
}

fn interior(outline: &Outline) -> Point {
    match outline {
        Outline::Circle { center, .. } => *center,
        Outline::Path(_) => {
            let window = outline_window(outline);
            let mid = Point::new(
                (window.x_min + window.x_max) / 2.0,
                (window.y_min + window.y_max) / 2.0,
            );
            if contains(outline, mid) {
                mid
            } else {
                match outline {
                    Outline::Path(path) => path.start,
                    Outline::Circle { center, .. } => *center,
                }
            }
        }
    }
}

fn segment_gap(a0: Point, a1: Point, b0: Point, b1: Point) -> (f64, Point) {
    let ab = Point::new(a1.x - a0.x, a1.y - a0.y);
    let cd = Point::new(b1.x - b0.x, b1.y - b0.y);
    let ac = Point::new(a0.x - b0.x, a0.y - b0.y);
    let uu = ab.x * ab.x + ab.y * ab.y;
    let vv = cd.x * cd.x + cd.y * cd.y;
    let uv = ab.x * cd.x + ab.y * cd.y;
    let uw = ab.x * ac.x + ab.y * ac.y;
    let vw = cd.x * ac.x + cd.y * ac.y;
    let denom = uu * vv - uv * uv;
    let (s, t) = if uu < 1e-12 || vv < 1e-12 || denom.abs() < 1e-12 {
        (0.0, 0.0)
    } else {
        let s = ((uv * vw - vv * uw) / denom).clamp(0.0, 1.0);
        let t = ((uv * s + vw) / vv).clamp(0.0, 1.0);
        (((uv * t - uw) / uu).clamp(0.0, 1.0), t)
    };
    let pa = Point::new(a0.x + ab.x * s, a0.y + ab.y * s);
    let pb = Point::new(b0.x + cd.x * t, b0.y + cd.y * t);
    let dx = pa.x - pb.x;
    let dy = pa.y - pb.y;
    (
        (dx * dx + dy * dy).sqrt(),
        Point::new((pa.x + pb.x) / 2.0, (pa.y + pb.y) / 2.0),
    )
}

fn distance_points(a: Point, b: Point) -> f64 {
    let dx = a.x - b.x;
    let dy = a.y - b.y;
    (dx * dx + dy * dy).sqrt()
}

fn footprint_names(footprints: &[FootprintName]) -> HashMap<String, String> {
    footprints
        .iter()
        .map(|footprint| (footprint.reference.clone(), footprint.name.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use pcbcraft_domain::geometry::{rectangle, ClearanceClass, PadForm, Shape, Source};

    fn pad(reference: &str, number: &str, net: &str, center: Point) -> Shape {
        Shape {
            source: Source::Pad,
            layer: "F.Cu".into(),
            reference: Some(reference.into()),
            pad: Some(number.into()),
            net: Some(net.into()),
            width: None,
            blocks: true,
            clearance_class: ClearanceClass::Copper {
                net: Some(net.into()),
            },
            clearance_mm: 0.0,
            mask_expansion_mm: 0.0,
            drill_length_mm: None,
            pad_form: Some(PadForm::Smd),
            text_height_mm: None,
            inset: false,
            inward: false,
            occupied: rectangle(center, 0.5, 0.4),
            clearance: rectangle(center, 0.5, 0.4),
        }
    }

    #[test]
    fn smd_pads_closer_than_0_15_mm_fail() {
        let left = pad("U4", "1", "+3V3", Point::new(0.0, 0.0));
        let right = pad("U4", "5", "GND", Point::new(0.63, 0.0));
        let board = BoardGeometry {
            copper_layers: vec![
                "F.Cu".into(),
                "In1.Cu".into(),
                "In2.Cu".into(),
                "B.Cu".into(),
            ],
            outer_copper_mm: Some(0.035),
            inner_copper_mm: Some(0.015),
            footprints: vec![FootprintName {
                reference: "U4".into(),
                name: "XDFN".into(),
            }],
            shapes: vec![left, right],
        };
        let lines = report(
            &board,
            &Spec {
                project: "board.kicad_pro".into(),
                mask_color: "green".into(),
                rails: Vec::new(),
                allow_overhang: Vec::new(),
                outer_copper_oz: None,
                inner_copper_oz: None,
            },
        )
        .unwrap();
        let spacing = lines
            .iter()
            .find(|line| line.name == "smd-pad-spacing")
            .unwrap();
        assert_eq!(spacing.status, TestStatus::Failed, "{spacing:?}");
        assert!(spacing.details.iter().any(|detail| detail.contains("U4")));
        let slots = lines
            .iter()
            .find(|line| line.name == "plated-slot")
            .unwrap();
        assert_eq!(slots.status, TestStatus::Passed);
    }

    #[test]
    fn a_two_layer_one_ounce_board_uses_a_0_10_mm_track_floor() {
        let board = BoardGeometry {
            copper_layers: vec!["F.Cu".into(), "B.Cu".into()],
            outer_copper_mm: Some(0.035),
            inner_copper_mm: None,
            footprints: Vec::new(),
            shapes: Vec::new(),
        };
        let floors = floors(
            &board,
            &Spec {
                project: "board.kicad_pro".into(),
                mask_color: "green".into(),
                rails: Vec::new(),
                allow_overhang: Vec::new(),
                outer_copper_oz: None,
                inner_copper_oz: None,
            },
        )
        .unwrap();
        assert!((floors.track_width_mm - 0.10).abs() < 1e-9);
        assert!((floors.track_space_mm - 0.10).abs() < 1e-9);
    }

    #[test]
    fn a_pad_inside_the_board_is_not_on_the_edge() {
        use pcbcraft_domain::geometry::boundary_gap;
        let board = rectangle(Point::new(70.0, 40.0), 140.0, 30.0);
        let pad = rectangle(Point::new(61.0, 39.0), 1.0, 0.5);
        let gap = boundary_gap(&board, &pad);
        assert!(gap.distance > 1.0, "{gap:?}");
        let rounded =
            pcbcraft_domain::geometry::round_rect(Point::new(61.0, 39.0), 1.0, 0.5, 0.1, 0.0);
        let gap = boundary_gap(&board, &rounded);
        assert!(gap.distance > 1.0, "round {gap:?}");
    }
}
