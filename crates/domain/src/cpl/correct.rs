//! Złożenie bazy i pliku pozycji w poprawiony CPL.

use std::collections::{BTreeMap, HashMap};

use anyhow::{Context, Result};

use super::db::{footprint_item, is_lcsc_part, Database, Hit};
use super::local::LocalDb;
use super::placement::{
    angular_separation, format_mm, format_number, jlc_rotation, position_delta, DEG_EPSILON,
    MM_EPSILON,
};
use super::table::{cell, layer_text, parse_mm, parse_side, Cpl, InputRow, PosFile};

#[derive(Debug)]
pub struct Change {
    pub designator: String,
    pub package: String,
    pub bottom: bool,
    pub rotation_from: f64,
    pub rotation_to: f64,
    pub x_from: f64,
    pub y_from: f64,
    pub x_to: f64,
    pub y_to: f64,
    pub rotation_changed: bool,
    pub position_changed: bool,
    pub rule: Option<Hit>,
}

impl std::fmt::Display for Change {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let side = if self.bottom { "bottom" } else { "top" };
        write!(f, "{:<8} {:<7} {}", self.designator, side, self.package)?;
        if self.rotation_changed {
            write!(
                f,
                "  angle {}° → {}°",
                format_number(self.rotation_from),
                format_number(self.rotation_to)
            )?;
        }
        if self.position_changed {
            write!(
                f,
                "  position {},{} → {},{}",
                format_number(self.x_from),
                format_number(self.y_from),
                format_number(self.x_to),
                format_number(self.y_to)
            )?;
        }
        if let Some(rule) = &self.rule {
            write!(f, "  ({})", rule_note(rule))?;
        } else {
            write!(f, "  (bottom mirror)")?;
        }
        Ok(())
    }
}

fn rule_note(rule: &Hit) -> String {
    let mut note = format!("{}, {}°", rule.key, rule.rotation);
    if rule.board_offset {
        if rule.offset_x != 0.0 {
            note.push_str(&format!(", X {} mm", format_number(rule.offset_x)));
        }
        if rule.offset_y != 0.0 {
            note.push_str(&format!(", Y {} mm", format_number(rule.offset_y)));
        }
    } else if rule.offset_x != 0.0 || rule.offset_y != 0.0 {
        note.push_str(&format!(
            ", offset {}/{} mm",
            format_number(rule.offset_x),
            format_number(rule.offset_y)
        ));
    }
    note.push_str(&format!(", match {}", rule.field));
    note
}

#[derive(Debug)]
pub struct Report {
    pub changes: Vec<Change>,
    pub warnings: Vec<String>,
    pub parts: usize,
    pub matched: usize,
    pub unmatched: BTreeMap<String, usize>,
}

impl Report {
    pub fn counts(&self) -> String {
        let mirror = self
            .changes
            .iter()
            .filter(|change| change.rule.is_none())
            .count();
        let ruled = self.changes.len() - mirror;
        let idle = self.matched.saturating_sub(ruled);
        let mut text = format!(
            "Changed parts: {} of {}. From a rule: {ruled}. Bottom mirror only: {mirror}.",
            self.changes.len(),
            self.parts
        );
        if idle > 0 {
            text.push_str(&format!(" Rule with no geometry change: {idle}."));
        }
        let unmatched_parts: usize = self.unmatched.values().sum();
        text.push_str(&format!(
            " No rule: {unmatched_parts} parts in {} footprints.",
            self.unmatched.len()
        ));
        text
    }
}

impl std::fmt::Display for Report {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for warning in &self.warnings {
            writeln!(f, "warning: {warning}")?;
        }
        for change in &self.changes {
            writeln!(f, "{change}")?;
        }
        writeln!(f, "{}", self.counts())?;
        if !self.unmatched.is_empty() {
            writeln!(f, "Footprints with no rule:")?;
            for (name, count) in &self.unmatched {
                writeln!(f, "  {count}× {name}")?;
            }
        }
        Ok(())
    }
}

pub fn correct(
    db: &Database,
    local: &LocalDb,
    pos: &PosFile,
    bom: &HashMap<String, String>,
) -> Result<(Cpl, Report)> {
    let mut header = vec![
        "Designator".to_string(),
        "Val".to_string(),
        "Package".to_string(),
        "Mid X".to_string(),
        "Mid Y".to_string(),
        "Rotation".to_string(),
        "Layer".to_string(),
    ];
    for idx in &pos.cols.extras {
        header.push(pos.header.get(*idx).cloned().unwrap_or_default());
    }

    let mut rows = Vec::with_capacity(pos.rows.len());
    let mut report = Report {
        changes: Vec::new(),
        warnings: Vec::new(),
        parts: 0,
        matched: 0,
        unmatched: BTreeMap::new(),
    };

    let mut used_local = vec![false; local.len()];
    for row in &pos.rows {
        rows.push(correct_row(
            db,
            local,
            &mut used_local,
            pos,
            bom,
            row,
            &mut report,
        )?);
    }
    for (index, used) in used_local.iter().enumerate() {
        if !used {
            report.warnings.push(format!(
                "local rule {} does not match any part",
                local.label(index)
            ));
        }
    }
    Ok((Cpl::new(header, rows), report))
}

fn correct_row(
    db: &Database,
    local: &LocalDb,
    used_local: &mut [bool],
    pos: &PosFile,
    bom: &HashMap<String, String>,
    row: &InputRow,
    report: &mut Report,
) -> Result<Vec<String>> {
    let cols = &pos.cols;
    let designator = cell(row, cols.designator).to_string();
    let value = cols
        .value
        .map(|idx| cell(row, idx))
        .unwrap_or("")
        .to_string();
    let package = cell(row, cols.package).to_string();
    let where_ = format!("wiersz {} ({designator})", row.line);

    let bottom = parse_side(cell(row, cols.layer))
        .with_context(|| format!("{where_}: nieznana warstwa „{}”", cell(row, cols.layer)))?;
    let rotation = parse_mm(cell(row, cols.rotation), "angle").context(where_.clone())?;
    let x = parse_mm(cell(row, cols.x), "position X").context(where_.clone())?;
    let y = parse_mm(cell(row, cols.y), "position Y").context(where_.clone())?;

    let row_lcsc = cols.lcsc.map(|idx| cell(row, idx)).unwrap_or("");
    if !row_lcsc.is_empty() && !is_lcsc_part(row_lcsc) {
        report
            .warnings
            .push(format!("{where_}: \"{row_lcsc}\" is not an LCSC number"));
    }
    let lcsc = if !row_lcsc.is_empty() {
        row_lcsc.to_string()
    } else {
        bom.get(&designator.to_ascii_uppercase())
            .cloned()
            .unwrap_or_default()
    };

    let remote = db
        .lookup(&designator, &value, &package, &lcsc)
        .with_context(|| where_.clone())?;
    let rule = if let Some(index) = local.lookup(&designator, &package) {
        used_local[index] = true;
        Some(local.hit(index))
    } else {
        remote
    };
    report.parts += 1;
    if rule.is_some() {
        report.matched += 1;
    } else {
        let name = footprint_item(&package);
        let name = if name.is_empty() { "(no name)" } else { name };
        *report.unmatched.entry(name.to_string()).or_insert(0) += 1;
    }

    let new_rotation = jlc_rotation(rotation, rule.as_ref().map(|hit| hit.rotation), bottom);
    let (dx, dy) = match &rule {
        Some(hit) if hit.board_offset => (hit.offset_x, hit.offset_y),
        Some(hit) if hit.offset_x.abs() > 0.0 || hit.offset_y.abs() > 0.0 => {
            position_delta(hit.offset_x, hit.offset_y, rotation, bottom)
        }
        _ => (0.0, 0.0),
    };
    let rotation_changed = angular_separation(rotation, new_rotation) > DEG_EPSILON;
    let x_changed = dx.abs() > MM_EPSILON;
    let y_changed = dy.abs() > MM_EPSILON;

    let rotation_text = if rotation_changed {
        format_mm(new_rotation)
    } else {
        cell(row, cols.rotation).to_string()
    };
    let x_text = if x_changed {
        format_mm(x + dx)
    } else {
        cell(row, cols.x).to_string()
    };
    let y_text = if y_changed {
        format_mm(y + dy)
    } else {
        cell(row, cols.y).to_string()
    };

    if rotation_changed || x_changed || y_changed {
        report.changes.push(Change {
            designator: designator.clone(),
            package: package.clone(),
            bottom,
            rotation_from: rotation,
            rotation_to: new_rotation,
            x_from: x,
            y_from: y,
            x_to: x + dx,
            y_to: y + dy,
            rotation_changed,
            position_changed: x_changed || y_changed,
            rule,
        });
    }

    let mut out = vec![
        designator,
        value,
        package,
        x_text,
        y_text,
        rotation_text,
        layer_text(cell(row, cols.layer), bottom),
    ];
    for idx in &cols.extras {
        out.push(row.cells.get(*idx).cloned().unwrap_or_default());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cpl::db::MatchField;

    fn run(db_csv: &str, pos_csv: &str, bom_csv: Option<&str>) -> (String, Report) {
        let db = Database::parse(db_csv).unwrap();
        let pos = PosFile::parse(pos_csv).unwrap();
        let (bom, warnings) = match bom_csv {
            Some(text) => parse_bom_for_test(text),
            None => (HashMap::new(), Vec::new()),
        };
        let (cpl, mut report) = correct(&db, &LocalDb::default(), &pos, &bom).unwrap();
        report.warnings.extend(warnings);
        (cpl.to_csv().unwrap(), report)
    }

    fn parse_bom_for_test(text: &str) -> (HashMap<String, String>, Vec<String>) {
        crate::cpl::parse_bom(text).unwrap()
    }

    #[test]
    fn corrects_rotation_and_mirrors_the_bottom() {
        let db = "pattern,rotation\n^SOT-23,-90\n";
        let pos = "\
Ref,Val,Package,PosX,PosY,Rot,Side
Q1,AO3401A,SOT-23,43.900000,-57.100000,180.000000,bottom
C1,100nF,C_0402_1005Metric,1.000000,2.000000,180.000000,bottom
C2,100nF,C_0402_1005Metric,3,4,0,top
D1,X,SOT-23,5,6,0,top
";
        let (csv, report) = run(db, pos, None);
        assert_eq!(
            csv,
            "\
Designator,Val,Package,Mid X,Mid Y,Rotation,Layer
Q1,AO3401A,SOT-23,43.900000,-57.100000,270.000000,bottom
C1,100nF,C_0402_1005Metric,1.000000,2.000000,0.000000,bottom
C2,100nF,C_0402_1005Metric,3,4,0,top
D1,X,SOT-23,5,6,270.000000,top
"
        );
        assert_eq!(report.parts, 4);
        assert_eq!(report.changes.len(), 3);
        assert!(report.changes.iter().any(|change| {
            change.designator == "Q1" && change.rule.as_ref().unwrap().key == "^SOT-23"
        }));
        assert!(report
            .changes
            .iter()
            .any(|change| change.designator == "C1" && change.rule.is_none()));
        assert_eq!(report.unmatched.get("C_0402_1005Metric"), Some(&2));
    }

    #[test]
    fn keeps_an_equivalent_angle_text() {
        let db = "pattern,rotation\n^SOT-23,0\n";
        let pos = "\
Ref,Val,Package,PosX,PosY,Rot,Side
Q1,X,SOT-23,1,2,-90,top
Q2,X,SOT-23,1,2,-90,bottom
";
        let (csv, report) = run(db, pos, None);
        assert!(csv.contains("Q1,X,SOT-23,1,2,-90,top"), "{csv}");
        assert!(csv.contains("Q2,X,SOT-23,1,2,-90,bottom"), "{csv}");
        assert!(report.changes.is_empty(), "{report:?}");
    }

    #[test]
    fn offset_uses_the_angle_before_the_correction() {
        let db = r#"pattern,rotation,offset_x,offset_y
^PinHeader_2x05_P1\.27mm_Vertical,90,2.54,0.635
"#;
        let pos = "\
Ref,Val,Package,PosX,PosY,Rot,Side
J1,HDR,PinHeader_2x05_P1.27mm_Vertical,10.000000,20.000000,0.000000,top
J2,HDR,PinHeader_2x05_P1.27mm_Vertical,10.000000,20.000000,90.000000,top
";
        let (csv, report) = run(db, pos, None);
        assert!(
            csv.contains(
                "J1,HDR,PinHeader_2x05_P1.27mm_Vertical,12.540000,19.365000,90.000000,top"
            ),
            "{csv}"
        );
        assert!(
            csv.contains(
                "J2,HDR,PinHeader_2x05_P1.27mm_Vertical,10.635000,22.540000,180.000000,top"
            ),
            "{csv}"
        );
        assert_eq!(report.changes.len(), 2);
    }

    #[test]
    fn lcsc_from_the_bom_wins_and_a_quoted_comma_survives() {
        let db = "pattern,rotation\nC99999,45\n^SOT-23,-90\n^Q1$,10\n";
        let pos = "\
Ref,Val,Package,PosX,PosY,Rot,Side
Q1,\"A, B\",SOT-23,1,2,0,top
";
        let bom = "Designator,LCSC Part #\nQ1,C99999\n";
        let (csv, report) = run(db, pos, Some(bom));
        assert_eq!(
            csv,
            "\
Designator,Val,Package,Mid X,Mid Y,Rotation,Layer
Q1,\"A, B\",SOT-23,1,2,45.000000,top
"
        );
        assert_eq!(
            report.changes[0].rule.as_ref().unwrap().field,
            MatchField::Lcsc
        );
    }

    #[test]
    fn semicolon_export_and_back_side_alias() {
        let db = "pattern,rotation\n^SOT-23,-90\n";
        let pos = "\
Ref;Val;Package;PosX;PosY;Rot;Side
Q1;X;SOT-23;1;2;0;top
Q2;X;C_0402;3;4;180;back
";
        let (csv, _) = run(db, pos, None);
        assert!(csv.contains("Q1,X,SOT-23,1,2,270.000000,top"), "{csv}");
        assert!(csv.contains("Q2,X,C_0402,3,4,0.000000,bottom"), "{csv}");
    }

    #[test]
    fn local_rule_replaces_the_remote_one_and_shifts_on_board_axes() {
        let db = Database::parse("pattern,rotation\n^(.*?_|V)?QFN-(16|20|24|28|40)(-|_|$),270\n")
            .unwrap();
        let local = LocalDb::parse(
            "\
transformations:
  - ref: U1
    rotation: 90
  - ref: J2
    x: -1.5
  - ref: BRAK
    y: 1
",
        )
        .unwrap();
        let pos = PosFile::parse(
            "\
Ref,Val,Package,PosX,PosY,Rot,Side
U1,ESP,QFN-40-1EP,10.000000,20.000000,0.000000,top
J2,BAT,Molex,10.000000,20.000000,90.000000,top
",
        )
        .unwrap();
        let (cpl, report) = correct(&db, &local, &pos, &HashMap::new()).unwrap();
        let csv = cpl.to_csv().unwrap();
        assert!(
            csv.contains("U1,ESP,QFN-40-1EP,10.000000,20.000000,90.000000,top"),
            "{csv}"
        );
        assert!(
            csv.contains("J2,BAT,Molex,8.500000,20.000000,90.000000,top"),
            "{csv}"
        );
        assert!(report
            .warnings
            .iter()
            .any(|warning| warning.contains("BRAK")));
        assert_eq!(
            report.changes[0].rule.as_ref().unwrap().field,
            crate::cpl::db::MatchField::LocalRef
        );
    }
}
