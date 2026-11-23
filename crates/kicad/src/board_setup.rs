//! Write a fabrication profile into the KiCad project.
//!
//! Numbers only go up. Custom rules named `pcbcraft …` are replaced. Every
//! other rule is left byte for byte. A copy is checked with `kicad-cli pcb drc`
//! before the new files are kept.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{bail, Context, Result};
use pcbcraft_domain::fab::Limits;

use crate::net::{atom_at, children, head, parse_sexp};

const CANARY: &str = "pcbcraft-load-check";
const PROBE_TRACK_MM: f64 = 20.0;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Change {
    pub label: String,
    pub from: String,
    pub to: String,
}

pub struct Copper {
    pub layers: u32,
    pub outer_mm: f64,
}

pub fn read_copper(board: &str) -> Result<Copper> {
    let expr = parse_sexp(board).context("board file")?;
    let mut layers = 0_u32;
    if let Some(list) = child(&expr, "layers") {
        for item in children(list).iter().skip(1) {
            let Some(name) = atom_at(item, 1) else {
                continue;
            };
            let kind = atom_at(item, 2).unwrap_or("");
            if kind == "signal" || kind == "power" || name.ends_with(".Cu") {
                layers += 1;
            }
        }
    }
    let front = layer_thickness(&expr, "F.Cu");
    let back = layer_thickness(&expr, "B.Cu");
    let outer_mm = match (front, back) {
        (Some(front), Some(back)) => front.max(back),
        (Some(front), None) => front,
        (None, Some(back)) => back,
        (None, None) => 0.035,
    };
    Ok(Copper {
        layers: layers.max(1),
        outer_mm,
    })
}

pub fn apply(
    pro_path: &Path,
    board_path: &Path,
    limits: &Limits,
    kicad_bin: &Path,
) -> Result<String> {
    let dru_path = pro_path.with_extension("kicad_dru");
    let pro =
        fs::read_to_string(pro_path).with_context(|| format!("read {}", pro_path.display()))?;
    let board =
        fs::read_to_string(board_path).with_context(|| format!("read {}", board_path.display()))?;
    let dru_existed = dru_path.is_file();
    let dru = if dru_existed {
        fs::read_to_string(&dru_path).with_context(|| format!("read {}", dru_path.display()))?
    } else {
        String::new()
    };

    let (pro_next, mut changes) = edit_project(&pro, limits)?;
    let (dru_next, dru_changed) = edit_rules(&dru, limits);
    let (board_next, board_changes) = edit_board(&board, limits)?;
    changes.extend(board_changes);

    let same = pro_next == pro && dru_next == dru && board_next == board;
    let header = format!(
        "{}  {} layers  {} oz  mask {}",
        limits.name,
        limits.layers,
        format_mm(limits.outer_oz),
        limits.mask
    );
    if same {
        return Ok(format!("{header}\nalready at the profile floors\n"));
    }

    let backup = Backup::capture(
        pro_path,
        board_path,
        dru_existed.then_some(dru_path.as_path()),
    )?;
    let wrote = (|| -> Result<()> {
        if pro_next != pro {
            fs::write(pro_path, &pro_next)?;
        }
        if board_next != board {
            fs::write(board_path, &board_next)?;
        }
        if dru_next != dru {
            fs::write(&dru_path, &dru_next)?;
        }
        probe(pro_path, board_path, &dru_path, kicad_bin)
    })();
    if let Err(err) = wrote {
        match backup.restore(pro_path, board_path, &dru_path) {
            Ok(()) => bail!("restored the board files\n{err:#}"),
            Err(restore) => {
                bail!("board setup failed and the backup restore failed: {restore:#}\nbackup: {}\n{err:#}", backup.dir.display())
            }
        }
    }
    backup.remove();

    let mut lines = vec![header];
    if changes.is_empty() && !dru_changed {
        lines.push("already at the profile floors".into());
    }
    for change in &changes {
        lines.push(format!(
            "raised {} {} -> {}",
            change.label, change.from, change.to
        ));
    }
    if dru_changed {
        lines.push(format!("wrote {}", dru_path.display()));
    }
    lines.push(String::new());
    Ok(lines.join("\n"))
}

fn probe(pro_path: &Path, board_path: &Path, dru_path: &Path, kicad_bin: &Path) -> Result<()> {
    let dir = scratch("probe");
    let stem = pro_path
        .file_stem()
        .context("project name")?
        .to_string_lossy()
        .into_owned();
    let pro = dir.join(format!("{stem}.kicad_pro"));
    let board = dir.join(format!("{stem}.kicad_pcb"));
    let dru = dir.join(format!("{stem}.kicad_dru"));
    fs::copy(pro_path, &pro)?;
    fs::copy(board_path, &board)?;
    fs::copy(dru_path, &dru)?;

    let mut pro_text = fs::read_to_string(&pro)?;
    let mut changes = Vec::new();
    set_number(
        &mut pro_text,
        &["board", "design_settings", "rules"],
        "min_track_width",
        PROBE_TRACK_MM,
        "probe",
        &mut changes,
    )?;
    fs::write(&pro, pro_text)?;

    let mut dru_text = fs::read_to_string(&dru)?;
    if !dru_text.ends_with('\n') {
        dru_text.push('\n');
    }
    dru_text.push_str(&format!(
        "(rule \"{CANARY}\"\n  (constraint assertion \"false\")\n)\n"
    ));
    fs::write(&dru, dru_text)?;

    let mut board_text = fs::read_to_string(&board)?;
    if !board_text.contains("(segment") && !board_text.contains("(arc") {
        insert_probe_segment(&mut board_text)?;
        fs::write(&board, board_text)?;
    }

    let report = dir.join("drc.json");
    let output = Command::new(kicad_bin)
        .args([
            "pcb",
            "drc",
            "--format",
            "json",
            "--severity-error",
            "--units",
            "mm",
            "--output",
            &report.display().to_string(),
            &board.display().to_string(),
        ])
        .output()
        .with_context(|| format!("cannot run {}", kicad_bin.display()))?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    let code = output.status.code();
    if code == Some(3) || !report.is_file() {
        let _ = fs::remove_dir_all(&dir);
        bail!("kicad-cli could not load the board (exit {code:?})\n{stderr}");
    }
    let text = fs::read_to_string(&report).unwrap_or_default();
    let _ = fs::remove_dir_all(&dir);
    let track_seen = text.contains("20,0000") || text.contains("20.0000");
    if text.contains(CANARY) && track_seen {
        return Ok(());
    }
    bail!("kicad-cli did not apply the project rules or the custom rules\n{stderr}{text}")
}

fn insert_probe_segment(board: &mut String) -> Result<()> {
    let end = board
        .rfind(')')
        .context("board has no closing parenthesis")?;
    board.insert_str(
        end,
        "\t(segment (start 0 0) (end 1 0) (width 0.5) (layer \"F.Cu\") (net 0))\n",
    );
    Ok(())
}

fn edit_project(text: &str, limits: &Limits) -> Result<(String, Vec<Change>)> {
    let mut text = text.to_string();
    let mut changes = Vec::new();
    let rules = ["board", "design_settings", "rules"];
    raise_number(
        &mut text,
        &rules,
        "min_clearance",
        limits.track_space_mm,
        "board.design_settings.rules.min_clearance",
        &mut changes,
    )?;
    raise_number(
        &mut text,
        &rules,
        "min_track_width",
        limits.track_width_mm,
        "board.design_settings.rules.min_track_width",
        &mut changes,
    )?;
    raise_number(
        &mut text,
        &rules,
        "min_connection",
        limits.track_width_mm,
        "board.design_settings.rules.min_connection",
        &mut changes,
    )?;
    if let Some(drill) = limits.min_drill_mm() {
        raise_number(
            &mut text,
            &rules,
            "min_through_hole_diameter",
            drill,
            "board.design_settings.rules.min_through_hole_diameter",
            &mut changes,
        )?;
    }
    if let Some(spacing) = limits.min_hole_spacing_mm() {
        raise_number(
            &mut text,
            &rules,
            "min_hole_to_hole",
            spacing,
            "board.design_settings.rules.min_hole_to_hole",
            &mut changes,
        )?;
    }
    if let Some(copper) = limits.min_hole_copper_mm() {
        raise_number(
            &mut text,
            &rules,
            "min_hole_clearance",
            copper,
            "board.design_settings.rules.min_hole_clearance",
            &mut changes,
        )?;
    }
    if let Some(edge) = limits.copper_to_edge_mm {
        raise_number(
            &mut text,
            &rules,
            "min_copper_edge_clearance",
            edge,
            "board.design_settings.rules.min_copper_edge_clearance",
            &mut changes,
        )?;
    }
    if let Some(ring) = limits.via_annular_mm() {
        raise_number(
            &mut text,
            &rules,
            "min_via_annular_width",
            ring,
            "board.design_settings.rules.min_via_annular_width",
            &mut changes,
        )?;
    }
    if let Some(via) = limits.via_diameter_mm() {
        raise_number(
            &mut text,
            &rules,
            "min_via_diameter",
            via,
            "board.design_settings.rules.min_via_diameter",
            &mut changes,
        )?;
    }
    if let Some(mask) = limits.mask_to_copper_mm {
        raise_number(
            &mut text,
            &rules,
            "solder_mask_to_copper_clearance",
            mask,
            "board.design_settings.rules.solder_mask_to_copper_clearance",
            &mut changes,
        )?;
    }
    if let Some(silk) = limits.silk_to_pad_mm {
        raise_number(
            &mut text,
            &rules,
            "min_silk_clearance",
            silk,
            "board.design_settings.rules.min_silk_clearance",
            &mut changes,
        )?;
    }

    let zones = ["board", "design_settings", "defaults", "zones"];
    if object_exists(&text, &zones) {
        raise_number(
            &mut text,
            &zones,
            "min_clearance",
            limits.track_space_mm,
            "board.design_settings.defaults.zones.min_clearance",
            &mut changes,
        )?;
        raise_number(
            &mut text,
            &zones,
            "min_thickness",
            limits.track_width_mm,
            "board.design_settings.defaults.zones.min_thickness",
            &mut changes,
        )?;
    }
    let defaults = ["board", "design_settings", "defaults"];
    if let Some(stroke) = limits.silk_stroke_mm {
        raise_number(
            &mut text,
            &defaults,
            "silk_line_width",
            stroke,
            "board.design_settings.defaults.silk_line_width",
            &mut changes,
        )?;
        raise_number(
            &mut text,
            &defaults,
            "silk_text_thickness",
            stroke,
            "board.design_settings.defaults.silk_text_thickness",
            &mut changes,
        )?;
    }
    if let Some(height) = limits.silk_height_mm {
        raise_number(
            &mut text,
            &defaults,
            "silk_text_size_h",
            height,
            "board.design_settings.defaults.silk_text_size_h",
            &mut changes,
        )?;
        raise_number(
            &mut text,
            &defaults,
            "silk_text_size_v",
            height,
            "board.design_settings.defaults.silk_text_size_v",
            &mut changes,
        )?;
    }

    let severities = ["board", "design_settings", "rule_severities"];
    bump(&mut text, &severities, "clearance", "error", &mut changes)?;
    bump(&mut text, &severities, "track_width", "error", &mut changes)?;
    bump(
        &mut text,
        &severities,
        "connection_width",
        "error",
        &mut changes,
    )?;
    if limits.via_annular_mm().is_some() || limits.annular_fail_mm.is_some() {
        bump(
            &mut text,
            &severities,
            "annular_width",
            "error",
            &mut changes,
        )?;
    }
    if limits.min_drill_mm().is_some() {
        bump(
            &mut text,
            &severities,
            "drill_out_of_range",
            "error",
            &mut changes,
        )?;
    }
    if limits.min_hole_copper_mm().is_some() {
        bump(
            &mut text,
            &severities,
            "hole_clearance",
            "error",
            &mut changes,
        )?;
    }
    if limits.min_hole_spacing_mm().is_some() {
        bump(
            &mut text,
            &severities,
            "hole_to_hole",
            "error",
            &mut changes,
        )?;
    }
    if limits.copper_to_edge_mm.is_some() {
        bump(
            &mut text,
            &severities,
            "copper_edge_clearance",
            "error",
            &mut changes,
        )?;
    }
    if limits.mask_bridge_mm.is_some() {
        bump(
            &mut text,
            &severities,
            "solder_mask_bridge",
            "error",
            &mut changes,
        )?;
    }
    if limits.silk_height_mm.is_some() {
        bump(
            &mut text,
            &severities,
            "text_height",
            "warning",
            &mut changes,
        )?;
    }
    if limits.silk_stroke_mm.is_some() {
        bump(
            &mut text,
            &severities,
            "text_thickness",
            "warning",
            &mut changes,
        )?;
    }
    if limits.silk_to_pad_mm.is_some() {
        bump(
            &mut text,
            &severities,
            "silk_over_copper",
            "warning",
            &mut changes,
        )?;
        bump(
            &mut text,
            &severities,
            "silk_edge_clearance",
            "warning",
            &mut changes,
        )?;
        bump(
            &mut text,
            &severities,
            "silk_overlap",
            "warning",
            &mut changes,
        )?;
    }

    raise_classes(&mut text, limits, &mut changes)?;
    Ok((text, changes))
}

fn edit_board(text: &str, limits: &Limits) -> Result<(String, Vec<Change>)> {
    let mut text = text.to_string();
    let mut changes = Vec::new();
    if let Some(mask) = limits.mask_to_copper_mm {
        raise_sexp(
            &mut text,
            "pad_to_mask_clearance",
            mask,
            "pad_to_mask_clearance",
            &mut changes,
        )?;
    }
    if let Some(bridge) = limits.mask_bridge_mm {
        if sexp_token_exists(&text, "solder_mask_min_width") {
            raise_sexp(
                &mut text,
                "solder_mask_min_width",
                bridge,
                "solder_mask_min_width",
                &mut changes,
            )?;
        } else {
            insert_after_sexp(
                &mut text,
                "pad_to_mask_clearance",
                &format!("(solder_mask_min_width {})", format_mm(bridge)),
            )?;
            changes.push(Change {
                label: "solder_mask_min_width".into(),
                from: "0".into(),
                to: format_mm(bridge),
            });
        }
    }
    Ok((text, changes))
}

pub(crate) fn edit_rules(text: &str, limits: &Limits) -> (String, bool) {
    let pieces = split_forms(text);
    let mut kept = String::new();
    let mut dropped_gap = false;
    for piece in &pieces {
        match piece {
            Piece::Gap(gap) => {
                if dropped_gap {
                    if gap.contains('#') {
                        kept.push_str(gap);
                    }
                    dropped_gap = false;
                } else {
                    kept.push_str(gap);
                }
            }
            Piece::Form { text, rule } => {
                if rule
                    .as_ref()
                    .is_some_and(|name| name.starts_with("pcbcraft "))
                {
                    dropped_gap = true;
                    continue;
                }
                kept.push_str(text);
            }
        }
    }
    if !kept.contains("(version") {
        kept = format!("(version 1)\n{kept}");
    }
    if !kept.ends_with('\n') {
        kept.push('\n');
    }
    let block = design_rules(limits);
    kept.push_str(&block);
    let changed = kept != text;
    (kept, changed)
}

fn design_rules(limits: &Limits) -> String {
    let mut out = String::new();
    let name = &limits.name;
    if let Some(ring) = limits.annular_fail_mm {
        out.push_str(&rule(
            name,
            "pth annular",
            None,
            &format!("annular_width (min {}mm)", format_mm(ring)),
            Some("A.Type == 'Pad' && A.Pad_Type == 'Through-hole'"),
        ));
    }
    if let Some(drill) = limits.npth_drill_fail_mm {
        if limits
            .min_drill_mm()
            .is_none_or(|global| drill > global + 1e-9)
        {
            out.push_str(&rule(
                name,
                "npth hole",
                None,
                &format!("hole_size (min {}mm)", format_mm(drill)),
                Some("A.Type == 'Pad' && A.Pad_Type == 'NPTH'"),
            ));
        }
    }
    if let Some(ring) = limits.npth_annular_warn_mm {
        out.push_str(&rule(
            name,
            "npth annular",
            Some("warning"),
            &format!("annular_width (min {}mm)", format_mm(ring)),
            Some("A.Type == 'Pad' && A.Pad_Type == 'NPTH'"),
        ));
    }
    if let (Some(pth), Some(global)) = (limits.pth_hole_spacing_mm, limits.min_hole_spacing_mm()) {
        if pth > global + 1e-9 {
            out.push_str(&rule(
                name,
                "pth hole spacing",
                None,
                &format!("hole_to_hole (min {}mm)", format_mm(pth)),
                Some("A.Type == 'Pad' && B.Type == 'Pad' && A.Pad_Type == 'Through-hole' && B.Pad_Type == 'Through-hole'"),
            ));
        }
    }
    if let (Some(outer), Some(global)) = (
        limits.pth_hole_copper_outer_fail_mm,
        limits.min_hole_copper_mm(),
    ) {
        if outer > global + 1e-9 {
            out.push_str(&rule(
                name,
                "pth hole to outer copper",
                None,
                &format!("hole_clearance (min {}mm)", format_mm(outer)),
                Some("A.Pad_Type == 'Through-hole' && (B.Layer == 'F.Cu' || B.Layer == 'B.Cu')"),
            ));
        }
    }
    if let Some(inner) = limits.pth_hole_copper_inner_mm {
        let outer = limits.pth_hole_copper_outer_fail_mm.unwrap_or(inner);
        let global = limits.min_hole_copper_mm().unwrap_or(0.0);
        if (inner - outer).abs() > 1e-9 && inner > global + 1e-9 {
            out.push_str(&rule(
                name,
                "pth hole to inner copper",
                None,
                &format!("hole_clearance (min {}mm)", format_mm(inner)),
                Some("A.Pad_Type == 'Through-hole' && B.Layer != 'F.Cu' && B.Layer != 'B.Cu'"),
            ));
        }
    }
    if let Some(gap) = limits.smd_pad_spacing_mm {
        if gap > limits.track_space_mm + 1e-9 {
            out.push_str(&rule(
                name,
                "smd pad spacing",
                None,
                &format!("clearance (min {}mm)", format_mm(gap)),
                Some("A.Type == 'Pad' && B.Type == 'Pad' && A.Pad_Type == 'SMD' && B.Pad_Type == 'SMD'"),
            ));
        }
    }
    if let Some(gap) = limits.smd_pad_to_track_mm {
        if gap > limits.track_space_mm + 1e-9 {
            out.push_str(&rule(
                name,
                "smd pad to track",
                None,
                &format!("clearance (min {}mm)", format_mm(gap)),
                Some("(A.Pad_Type == 'SMD' && B.Type == 'Track') || (B.Pad_Type == 'SMD' && A.Type == 'Track')"),
            ));
        }
    }
    let silk = "A.Layer == 'F.SilkS' || A.Layer == 'B.SilkS'";
    if let Some(height) = limits.silk_height_mm {
        out.push_str(&rule(
            name,
            "silk height",
            Some("warning"),
            &format!("text_height (min {}mm)", format_mm(height)),
            Some(silk),
        ));
    }
    if let Some(stroke) = limits.silk_stroke_mm {
        // `line_width` is not a constraint in KiCad 10.0.6. Emitting it makes
        // KiCad discard the whole rules file, so text thickness covers the stroke.
        out.push_str(&rule(
            name,
            "silk thickness",
            Some("warning"),
            &format!("text_thickness (min {}mm)", format_mm(stroke)),
            Some(silk),
        ));
    }
    if let Some(gap) = limits.silk_to_pad_mm {
        out.push_str(&rule(
            name,
            "silk clearance",
            Some("warning"),
            &format!("silk_clearance (min {}mm)", format_mm(gap)),
            Some(silk),
        ));
    }
    out
}

fn rule(
    profile: &str,
    title: &str,
    severity: Option<&str>,
    constraint: &str,
    condition: Option<&str>,
) -> String {
    let mut out = format!("(rule \"pcbcraft {profile} {title}\"\n");
    if let Some(severity) = severity {
        out.push_str(&format!("  (severity {severity})\n"));
    }
    out.push_str(&format!("  (constraint {constraint})\n"));
    if let Some(condition) = condition {
        out.push_str(&format!("  (condition \"{condition}\")\n"));
    }
    out.push_str(")\n");
    out
}

fn raise_classes(text: &mut String, limits: &Limits, changes: &mut Vec<Change>) -> Result<()> {
    let array = match find_value(text, &["net_settings", "classes"]) {
        Ok(span) => span,
        Err(_) => return Ok(()),
    };
    let mut names = Vec::new();
    let mut index = array.0 + 1;
    while index < array.1 {
        index = skip_ws(text, index);
        if text.as_bytes().get(index) == Some(&b',') {
            index += 1;
            continue;
        }
        if index >= array.1 || text.as_bytes().get(index) == Some(&b']') {
            break;
        }
        if text.as_bytes().get(index) != Some(&b'{') {
            bail!("net class is not an object");
        }
        let end = end_of_value(text, index)?;
        let body = &text[index..end];
        if let Some(name) = object_string(body, "name") {
            names.push(name);
        }
        index = end;
    }
    for name in names {
        let label = |field: &str| format!("net_settings.classes[{name}].{field}");
        raise_number_if_present(
            text,
            &["net_settings", "classes"],
            &name,
            "clearance",
            limits.track_space_mm,
            &label("clearance"),
            changes,
        )?;
        raise_number_if_present(
            text,
            &["net_settings", "classes"],
            &name,
            "track_width",
            limits.track_width_mm,
            &label("track_width"),
            changes,
        )?;
        if let Some(drill) = limits.via_drill_fail_mm {
            raise_number_if_present(
                text,
                &["net_settings", "classes"],
                &name,
                "via_drill",
                drill,
                &label("via_drill"),
                changes,
            )?;
        }
        if let Some(via) = limits.via_diameter_mm() {
            raise_number_if_present(
                text,
                &["net_settings", "classes"],
                &name,
                "via_diameter",
                via,
                &label("via_diameter"),
                changes,
            )?;
        }
        raise_number_if_present(
            text,
            &["net_settings", "classes"],
            &name,
            "diff_pair_width",
            limits.track_width_mm,
            &label("diff_pair_width"),
            changes,
        )?;
        raise_number_if_present(
            text,
            &["net_settings", "classes"],
            &name,
            "diff_pair_gap",
            limits.track_space_mm,
            &label("diff_pair_gap"),
            changes,
        )?;
    }
    Ok(())
}

fn raise_number_if_present(
    text: &mut String,
    array_path: &[&str],
    class_name: &str,
    key: &str,
    floor: f64,
    label: &str,
    changes: &mut Vec<Change>,
) -> Result<()> {
    let Some(span) = class_object(text, array_path, class_name)? else {
        return Ok(());
    };
    let body = &text[span.0..span.1];
    if object_number_token(body, key).is_none() {
        return Ok(());
    }
    raise_inside(text, span, key, floor, label, changes, false)
}

fn class_object(text: &str, path: &[&str], name: &str) -> Result<Option<(usize, usize)>> {
    let array = find_value(text, path)?;
    let mut index = array.0 + 1;
    while index < array.1 {
        index = skip_ws(text, index);
        if text.as_bytes().get(index) == Some(&b',') {
            index += 1;
            continue;
        }
        if index >= array.1 || text.as_bytes().get(index) == Some(&b']') {
            break;
        }
        let end = end_of_value(text, index)?;
        let body = &text[index..end];
        if object_string(body, "name").as_deref() == Some(name) {
            return Ok(Some((index, end)));
        }
        index = end;
    }
    Ok(None)
}

fn raise_number(
    text: &mut String,
    path: &[&str],
    key: &str,
    floor: f64,
    label: &str,
    changes: &mut Vec<Change>,
) -> Result<()> {
    let object = find_value(text, path)?;
    raise_inside(text, object, key, floor, label, changes, true)
}

fn set_number(
    text: &mut String,
    path: &[&str],
    key: &str,
    value: f64,
    label: &str,
    changes: &mut Vec<Change>,
) -> Result<()> {
    let object = find_value(text, path)?;
    force_inside(text, object, key, value, label, changes)
}

fn raise_inside(
    text: &mut String,
    object: (usize, usize),
    key: &str,
    floor: f64,
    label: &str,
    changes: &mut Vec<Change>,
    insert_missing: bool,
) -> Result<()> {
    let body = text[object.0..object.1].to_string();
    if let Some((relative, token)) = object_number_token(&body, key) {
        let current: f64 = token
            .parse()
            .with_context(|| format!("{label} is not a number"))?;
        if current + 1e-9 >= floor {
            return Ok(());
        }
        let start = object.0 + relative;
        let end = start + token.len();
        let next = format_mm(floor);
        changes.push(Change {
            label: label.to_string(),
            from: token,
            to: next.clone(),
        });
        text.replace_range(start..end, &next);
        return Ok(());
    }
    if !insert_missing {
        return Ok(());
    }
    let next = format_mm(floor);
    insert_key(text, object, key, &next)?;
    changes.push(Change {
        label: label.to_string(),
        from: "missing".into(),
        to: next,
    });
    Ok(())
}

fn force_inside(
    text: &mut String,
    object: (usize, usize),
    key: &str,
    value: f64,
    label: &str,
    changes: &mut Vec<Change>,
) -> Result<()> {
    let body = text[object.0..object.1].to_string();
    let next = format_mm(value);
    if let Some((relative, token)) = object_number_token(&body, key) {
        let start = object.0 + relative;
        let end = start + token.len();
        if token == next {
            return Ok(());
        }
        changes.push(Change {
            label: label.to_string(),
            from: token,
            to: next.clone(),
        });
        text.replace_range(start..end, &next);
        return Ok(());
    }
    insert_key(text, object, key, &next)?;
    Ok(())
}

fn bump(
    text: &mut String,
    path: &[&str],
    key: &str,
    at_least: &str,
    changes: &mut Vec<Change>,
) -> Result<()> {
    let object = match find_value(text, path) {
        Ok(span) => span,
        Err(_) => return Ok(()),
    };
    let body = text[object.0..object.1].to_string();
    let target = rank(at_least);
    if let Some((relative, token)) = object_string_token(&body, key) {
        let current = rank(&token);
        if current >= target {
            return Ok(());
        }
        let start = object.0 + relative;
        let end = start + token.len();
        changes.push(Change {
            label: format!("{}.{}", path.join("."), key),
            from: token,
            to: at_least.to_string(),
        });
        text.replace_range(start..end, at_least);
        return Ok(());
    }
    insert_key(text, object, key, &format!("\"{at_least}\""))?;
    changes.push(Change {
        label: format!("{}.{}", path.join("."), key),
        from: "missing".into(),
        to: at_least.to_string(),
    });
    Ok(())
}

fn rank(value: &str) -> u8 {
    match value {
        "error" => 2,
        "warning" => 1,
        _ => 0,
    }
}

fn insert_key(text: &mut String, object: (usize, usize), key: &str, literal: &str) -> Result<()> {
    let end = object.1;
    let close = end
        .checked_sub(1)
        .filter(|index| text.as_bytes().get(*index) == Some(&b'}'))
        .context("object does not end with }")?;
    let interior = text[object.0 + 1..close].trim();
    let piece = if interior.is_empty() {
        format!("\"{key}\": {literal}")
    } else {
        format!(", \"{key}\": {literal}")
    };
    text.insert_str(close, &piece);
    Ok(())
}

fn object_exists(text: &str, path: &[&str]) -> bool {
    find_value(text, path).is_ok()
}

fn find_value(text: &str, path: &[&str]) -> Result<(usize, usize)> {
    let mut span = (skip_ws(text, 0), text.len());
    if text.as_bytes().get(span.0) != Some(&b'{') {
        bail!("project JSON must be an object");
    }
    span.1 = end_of_value(text, span.0)?;
    for key in path {
        let body_start = span.0;
        let found = object_key_span(text, span, key)?;
        let Some(next) = found else {
            bail!("missing {key} under the project");
        };
        if next.0 < body_start {
            bail!("missing {key}");
        }
        span = next;
    }
    Ok(span)
}

fn object_key_span(
    text: &str,
    object: (usize, usize),
    key: &str,
) -> Result<Option<(usize, usize)>> {
    let mut index = object.0 + 1;
    while index < object.1 {
        index = skip_ws(text, index);
        if index >= object.1 || text.as_bytes().get(index) == Some(&b'}') {
            return Ok(None);
        }
        let (name, after_key) = parse_string(text, index)?;
        index = skip_ws(text, after_key);
        if text.as_bytes().get(index) != Some(&b':') {
            bail!("expected ':' after {name}");
        }
        index = skip_ws(text, index + 1);
        let end = end_of_value(text, index)?;
        if name == key {
            return Ok(Some((index, end)));
        }
        index = skip_ws(text, end);
        if text.as_bytes().get(index) == Some(&b',') {
            index += 1;
        }
    }
    Ok(None)
}

fn object_number_token(object_text: &str, key: &str) -> Option<(usize, String)> {
    let span = object_key_span(object_text, (0, object_text.len()), key).ok()??;
    let token = object_text[span.0..span.1].trim().to_string();
    if token.starts_with('"') {
        return None;
    }
    Some((
        span.0 + (object_text[span.0..].len() - object_text[span.0..].trim_start().len()),
        token,
    ))
}

fn object_string_token(object_text: &str, key: &str) -> Option<(usize, String)> {
    let span = object_key_span(object_text, (0, object_text.len()), key).ok()??;
    let raw = object_text[span.0..span.1].trim();
    let value = raw.trim_matches('"').to_string();
    let start = span.0 + (object_text[span.0..].len() - object_text[span.0..].trim_start().len());
    let inner = start + 1;
    Some((inner, value))
}

fn object_string(object_text: &str, key: &str) -> Option<String> {
    object_string_token(object_text, key).map(|(_, value)| value)
}

fn end_of_value(text: &str, start: usize) -> Result<usize> {
    let bytes = text.as_bytes();
    match bytes.get(start) {
        Some(b'{') => end_of_container(text, start, b'{', b'}'),
        Some(b'[') => end_of_container(text, start, b'[', b']'),
        Some(b'"') => {
            let (_, end) = parse_string(text, start)?;
            Ok(end)
        }
        Some(_) => {
            let mut index = start;
            while index < text.len()
                && !matches!(
                    bytes[index],
                    b',' | b'}' | b']' | b' ' | b'\n' | b'\r' | b'\t'
                )
            {
                index += 1;
            }
            Ok(index)
        }
        None => bail!("truncated JSON"),
    }
}

fn end_of_container(text: &str, start: usize, open: u8, close: u8) -> Result<usize> {
    let bytes = text.as_bytes();
    let mut depth = 0_i32;
    let mut index = start;
    while index < text.len() {
        let byte = bytes[index];
        if byte == b'"' {
            let (_, end) = parse_string(text, index)?;
            index = end;
            continue;
        }
        if byte == open {
            depth += 1;
        } else if byte == close {
            depth -= 1;
            if depth == 0 {
                return Ok(index + 1);
            }
        }
        index += 1;
    }
    bail!("unclosed JSON value")
}

fn parse_string(text: &str, start: usize) -> Result<(String, usize)> {
    let bytes = text.as_bytes();
    if bytes.get(start) != Some(&b'"') {
        bail!("expected a string at {start}");
    }
    let mut out = String::new();
    let mut index = start + 1;
    while index < text.len() {
        let byte = bytes[index];
        if byte == b'\\' {
            if let Some(next) = bytes.get(index + 1) {
                out.push(*next as char);
                index += 2;
                continue;
            }
        }
        if byte == b'"' {
            return Ok((out, index + 1));
        }
        out.push(byte as char);
        index += 1;
    }
    bail!("unclosed string")
}

fn skip_ws(text: &str, mut index: usize) -> usize {
    while text
        .as_bytes()
        .get(index)
        .is_some_and(u8::is_ascii_whitespace)
    {
        index += 1;
    }
    index
}

fn raise_sexp(
    text: &mut String,
    token: &str,
    floor: f64,
    label: &str,
    changes: &mut Vec<Change>,
) -> Result<()> {
    let Some((start, end, current)) = sexp_number(text, token) else {
        return Ok(());
    };
    if current + 1e-9 >= floor {
        return Ok(());
    }
    let next = format_mm(floor);
    changes.push(Change {
        label: label.to_string(),
        from: text[start..end].to_string(),
        to: next.clone(),
    });
    text.replace_range(start..end, &next);
    Ok(())
}

fn sexp_token_exists(text: &str, token: &str) -> bool {
    sexp_number(text, token).is_some()
}

fn sexp_number(text: &str, token: &str) -> Option<(usize, usize, f64)> {
    let needle = format!("({token}");
    let mut from = 0;
    while let Some(found) = text[from..].find(&needle) {
        let index = from + found + needle.len();
        let rest = text[index..].trim_start();
        let skipped = text[index..].len() - rest.len();
        let number: String = rest
            .chars()
            .take_while(|c| c.is_ascii_digit() || *c == '.' || *c == '-')
            .collect();
        if let Ok(value) = number.parse::<f64>() {
            let start = index + skipped;
            return Some((start, start + number.len(), value));
        }
        from = index;
    }
    None
}

fn insert_after_sexp(text: &mut String, token: &str, insertion: &str) -> Result<()> {
    let needle = format!("({token}");
    let start = text
        .find(&needle)
        .with_context(|| format!("missing ({token}"))?;
    let mut depth = 0_i32;
    let bytes = text.as_bytes();
    let mut index = start;
    let mut in_string = false;
    while index < text.len() {
        let byte = bytes[index];
        if byte == b'"' && (index == 0 || bytes[index - 1] != b'\\') {
            in_string = !in_string;
        }
        if !in_string {
            if byte == b'(' {
                depth += 1;
            } else if byte == b')' {
                depth -= 1;
                if depth == 0 {
                    text.insert_str(index + 1, &format!("\n\t\t{insertion}"));
                    return Ok(());
                }
            }
        }
        index += 1;
    }
    bail!("unclosed ({token}")
}

enum Piece {
    Gap(String),
    Form { text: String, rule: Option<String> },
}

fn split_forms(text: &str) -> Vec<Piece> {
    let mut pieces = Vec::new();
    let mut index = 0;
    let chars: Vec<char> = text.chars().collect();
    while index < chars.len() {
        if chars[index].is_whitespace() || chars[index] == '#' {
            let start = index;
            while index < chars.len() && chars[index] != '(' {
                if chars[index] == '#' {
                    while index < chars.len() && chars[index] != '\n' {
                        index += 1;
                    }
                } else if chars[index].is_whitespace() {
                    index += 1;
                } else {
                    break;
                }
            }
            pieces.push(Piece::Gap(chars[start..index].iter().collect()));
            continue;
        }
        if chars[index] == '(' {
            let start = index;
            let mut depth = 0_i32;
            let mut in_string = false;
            while index < chars.len() {
                let ch = chars[index];
                if ch == '"' && (index == 0 || chars[index - 1] != '\\') {
                    in_string = !in_string;
                }
                if !in_string {
                    if ch == '(' {
                        depth += 1;
                    } else if ch == ')' {
                        depth -= 1;
                        index += 1;
                        if depth == 0 {
                            break;
                        }
                        continue;
                    }
                }
                index += 1;
            }
            let form: String = chars[start..index].iter().collect();
            let rule = rule_name(&form);
            pieces.push(Piece::Form { text: form, rule });
            continue;
        }
        let start = index;
        index += 1;
        pieces.push(Piece::Gap(chars[start..index].iter().collect()));
    }
    pieces
}

fn rule_name(form: &str) -> Option<String> {
    let trimmed = form.trim_start();
    let rest = trimmed.strip_prefix("(rule")?.trim_start();
    let rest = rest.strip_prefix('"')?;
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

fn format_mm(value: f64) -> String {
    let rounded = (value * 10000.0).round() / 10000.0;
    let mut text = format!("{rounded:.4}");
    if text.contains('.') {
        while text.ends_with('0') {
            text.pop();
        }
        if text.ends_with('.') {
            text.pop();
        }
    }
    text
}

fn child<'a>(expr: &'a crate::net::SExpr, name: &str) -> Option<&'a crate::net::SExpr> {
    children(expr).iter().find(|item| head(item) == Some(name))
}

fn layer_thickness(expr: &crate::net::SExpr, name: &str) -> Option<f64> {
    fn walk(expr: &crate::net::SExpr, name: &str) -> Option<f64> {
        if head(expr) == Some("layer") && atom_at(expr, 1) == Some(name) {
            let thick = child(expr, "thickness")?;
            return atom_at(thick, 1)?.parse().ok();
        }
        for item in children(expr).iter().skip(1) {
            if let Some(found) = walk(item, name) {
                return Some(found);
            }
        }
        None
    }
    walk(expr, name)
}

struct Backup {
    dir: PathBuf,
    dru_existed: bool,
}

impl Backup {
    fn capture(pro: &Path, board: &Path, dru: Option<&Path>) -> Result<Self> {
        let dir = scratch("backup");
        fs::copy(pro, dir.join(pro.file_name().context("pro name")?))?;
        fs::copy(board, dir.join(board.file_name().context("board name")?))?;
        let dru_existed = if let Some(dru) = dru {
            fs::copy(dru, dir.join(dru.file_name().context("dru name")?))?;
            true
        } else {
            false
        };
        Ok(Self { dir, dru_existed })
    }

    fn restore(&self, pro: &Path, board: &Path, dru: &Path) -> Result<()> {
        fs::copy(self.dir.join(pro.file_name().context("pro")?), pro)?;
        fs::copy(self.dir.join(board.file_name().context("board")?), board)?;
        if self.dru_existed {
            fs::copy(self.dir.join(dru.file_name().context("dru")?), dru)?;
        } else if dru.exists() {
            fs::remove_file(dru)?;
        }
        self.remove();
        Ok(())
    }

    fn remove(&self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

fn scratch(label: &str) -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "pcbcraft-board-setup-{label}-{}-{n}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(&path).ok();
    path
}

#[cfg(test)]
mod tests {
    use super::*;
    use pcbcraft_domain::fab::{jlcpcb, limits};

    fn sample_pro() -> String {
        r#"{
  "board": {
    "design_settings": {
      "defaults": {
        "silk_line_width": 0.1,
        "silk_text_size_h": 1.0,
        "silk_text_size_v": 1.0,
        "silk_text_thickness": 0.1,
        "zones": { "min_clearance": 0.0, "min_thickness": 0.2 }
      },
      "rule_severities": {
        "clearance": "ignore",
        "hole_to_hole": "warning",
        "track_width": "error",
        "text_height": "ignore"
      },
      "rules": {
        "min_clearance": 0.2,
        "min_track_width": 0.0,
        "min_connection": 0.0,
        "min_hole_to_hole": 0.25,
        "min_silk_clearance": 0.0
      }
    }
  },
  "net_settings": {
    "classes": [
      {
        "name": "Default",
        "clearance": 0.05,
        "track_width": 0.2,
        "via_diameter": 0.6,
        "via_drill": 0.3
      },
      { "name": "Power", "clearance": 0.2, "track_width": 0.5 }
    ]
  }
}
"#
        .to_string()
    }

    fn sample_board() -> &'static str {
        r#"(kicad_pcb
  (layers
    (0 "F.Cu" signal)
    (2 "B.Cu" signal))
  (setup
    (stackup
      (layer "F.Cu" (type "copper") (thickness 0.035))
      (layer "B.Cu" (type "copper") (thickness 0.035)))
    (pad_to_mask_clearance 0))
  (segment (start 0 0) (end 10 0) (width 0.2) (layer "F.Cu") (net 0)))
"#
    }

    fn two_layer() -> Limits {
        limits(&jlcpcb(), 2, 0.035, "green").unwrap()
    }

    #[test]
    fn copper_uses_the_thicker_outer_layer() {
        let copper = read_copper(sample_board()).unwrap();
        assert_eq!(copper.layers, 2);
        assert!((copper.outer_mm - 0.035).abs() < 1e-9);
    }

    #[test]
    fn floors_only_rise_and_a_second_pass_is_identical() {
        let limits = two_layer();
        let (once, changes) = edit_project(&sample_pro(), &limits).unwrap();
        assert!(changes
            .iter()
            .any(|change| change.label.ends_with("min_track_width") && change.to == "0.1"));
        assert!(changes
            .iter()
            .all(|change| change.label != "board.design_settings.rules.min_clearance"));
        assert!(once.contains("\"hole_to_hole\": \"error\""));
        assert!(once.contains("\"track_width\": \"error\""));
        assert!(once.contains("\"text_height\": \"warning\""));
        let (twice, again) = edit_project(&once, &limits).unwrap();
        assert!(again.is_empty(), "{again:?}");
        assert_eq!(once, twice);
        assert!(once.contains("\"Power\""));
        assert!(once.contains("\"track_width\": 0.5"));
    }

    #[test]
    fn user_rules_stay_and_old_pcbcraft_rules_are_replaced() {
        let original = "(version 1)\n\n(rule \"U4 pady\"\n  (constraint clearance (min 0.12mm))\n  (condition \"A.Type == 'Pad'\"))\n\n(rule \"pcbcraft old spacing\"\n  (constraint clearance (min 0.01mm)))\n";
        let limits = two_layer();
        let (once, _) = edit_rules(original, &limits);
        assert!(once.contains("(rule \"U4 pady\""));
        assert!(once.contains("0.12mm"));
        assert!(!once.contains("pcbcraft old"));
        assert!(once.contains("pcbcraft jlcpcb"));
        let (twice, changed) = edit_rules(&once, &limits);
        assert!(!changed);
        assert_eq!(once, twice);
    }

    #[test]
    fn a_failed_probe_restores_the_backup_and_a_passing_probe_keeps_the_raise() {
        let dir = std::env::temp_dir().join(format!("pcbcraft-setup-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let pro = dir.join("radio.kicad_pro");
        let board = dir.join("radio.kicad_pcb");
        fs::write(&pro, sample_pro()).unwrap();
        fs::write(&board, sample_board()).unwrap();
        let original_pro = fs::read_to_string(&pro).unwrap();
        let script = dir.join("kicad-cli");
        fs::write(&script, "#!/bin/sh\nexit 3\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut permissions = fs::metadata(&script).unwrap().permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(&script, permissions).unwrap();
        }
        let err = apply(&pro, &board, &two_layer(), &script).unwrap_err();
        assert!(err.to_string().contains("restored"), "{err}");
        assert_eq!(fs::read_to_string(&pro).unwrap(), original_pro);
        assert!(!dir.join("radio.kicad_dru").exists());

        fs::write(
            &script,
            r#"#!/bin/sh
out=""
prev=""
for arg in "$@"; do
  if [ "$prev" = "--output" ]; then out=$arg; fi
  prev=$arg
done
printf '%s\n' '{"violations":[{"description":"pcbcraft-load-check 20,0000 mm"}]}' > "$out"
exit 0
"#,
        )
        .unwrap();
        let ok = apply(&pro, &board, &two_layer(), &script).unwrap();
        assert!(ok.contains("raised"), "{ok}");
        assert!(fs::read_to_string(&pro)
            .unwrap()
            .contains("\"min_track_width\": 0.1"));
        assert!(dir.join("radio.kicad_dru").is_file());
        let again = apply(&pro, &board, &two_layer(), &script).unwrap();
        assert!(again.contains("already at the profile floors"), "{again}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_silent_rules_drop_restores_the_files() {
        let dir = std::env::temp_dir().join(format!("pcbcraft-setup-drop-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let pro = dir.join("radio.kicad_pro");
        let board = dir.join("radio.kicad_pcb");
        fs::write(&pro, sample_pro()).unwrap();
        fs::write(&board, sample_board()).unwrap();
        let script = dir.join("kicad-cli");
        fs::write(
            &script,
            r#"#!/bin/sh
out=""
prev=""
for arg in "$@"; do
  if [ "$prev" = "--output" ]; then out=$arg; fi
  prev=$arg
done
printf '%s\n' '{"violations":[]}' > "$out"
exit 0
"#,
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut permissions = fs::metadata(&script).unwrap().permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(&script, permissions).unwrap();
        }
        let err = apply(&pro, &board, &two_layer(), &script).unwrap_err();
        assert!(err.to_string().contains("restored"), "{err}");
        assert!(!fs::read_to_string(&pro)
            .unwrap()
            .contains("\"min_track_width\": 0.1"));
        let _ = fs::remove_dir_all(&dir);
    }
}
