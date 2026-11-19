//! Mounting holes and fiducials stay out of the BOM, the position file, and the catalogs.

use std::path::Path;

use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json::Value;

use super::scheduler::TestContext;
use super::validator::{project_schema, ready_jobs, Job, Line, ValidatorInterface};
use pcbcraft_domain::net::Component;

pub struct Attributes;

#[derive(Deserialize)]
struct Spec {
    project: String,
}

impl ValidatorInterface for Attributes {
    fn type_name(&self) -> &'static str {
        "Attributes"
    }

    fn schema(&self) -> Value {
        project_schema()
    }

    fn run(&self, file_name: &str, spec: &Value, ctx: &TestContext<'_>) -> Result<Vec<Job>> {
        let spec: Spec = serde_json::from_value(spec.clone()).context("spec Attributes")?;
        let graph = ctx.graph.load(Path::new(&spec.project))?;
        Ok(ready_jobs(file_name, lines(file_name, &graph.components)))
    }
}

fn lines(file_name: &str, components: &[Component]) -> Vec<Line> {
    let mut found = Vec::new();
    for component in components {
        if !is_mounting_hole_or_fiducial(&component.footprint) {
            continue;
        }
        let problems = problems(component);
        found.push(Line {
            name: component.reference.clone(),
            subtest: true,
            status: if problems.is_empty() {
                crate::ports::TestStatus::Passed
            } else {
                crate::ports::TestStatus::Failed
            },
            details: problems,
            assertions: 1,
        });
    }
    if found.is_empty() {
        return vec![Line {
            name: file_name.to_string(),
            subtest: false,
            status: crate::ports::TestStatus::Passed,
            details: vec!["no mounting holes or fiducials".into()],
            assertions: 1,
        }];
    }
    found
}

fn problems(component: &Component) -> Vec<String> {
    let mut problems = Vec::new();
    if component.in_bom {
        problems.push("in BOM".into());
    }
    if component.in_pos_files {
        problems.push("in position file".into());
    }
    if let Some(part) = component.lcsc.as_deref().filter(|part| !part.is_empty()) {
        problems.push(format!("LCSC {part}"));
    }
    if let Some(part) = component.pcbway.as_deref().filter(|part| !part.is_empty()) {
        problems.push(format!("PCBWay {part}"));
    }
    problems
}

/// `MountingHole:MountingHole_3.2mm_M3` and `Fiducial:Fiducial_1mm_Mask2mm`.
fn is_mounting_hole_or_fiducial(footprint: &str) -> bool {
    let footprint = footprint.trim();
    if footprint.is_empty() {
        return false;
    }
    let (library, name) = footprint.split_once(':').unwrap_or(("", footprint));
    matches_mark(library) || matches_mark(name)
}

fn matches_mark(text: &str) -> bool {
    let text = text.trim().to_ascii_lowercase();
    text == "mountinghole"
        || text == "fiducial"
        || text.starts_with("mountinghole")
        || text.starts_with("fiducial")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hole(reference: &str, footprint: &str) -> Component {
        Component {
            reference: reference.into(),
            symbol: "Mechanical:MountingHole".into(),
            value: "MountingHole".into(),
            footprint: footprint.into(),
            description: String::new(),
            lcsc: None,
            pcbway: None,
            dnp: false,
            in_bom: false,
            in_pos_files: false,
        }
    }

    #[test]
    fn a_correct_hole_passes_and_a_listed_one_fails() {
        let mut bad = hole("H2", "MountingHole:MountingHole_3.2mm_M3");
        bad.in_bom = true;
        bad.in_pos_files = true;
        bad.lcsc = Some("C1".into());
        let lines = lines(
            "attr",
            &[
                hole("H1", "MountingHole:MountingHole_3.2mm_M3"),
                bad,
                hole("R1", "Resistor_SMD:R_0805"),
            ],
        );
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].status, crate::ports::TestStatus::Passed);
        assert_eq!(lines[0].name, "H1");
        assert_eq!(lines[1].status, crate::ports::TestStatus::Failed);
        assert!(lines[1].details.iter().any(|item| item == "in BOM"));
        assert!(lines[1]
            .details
            .iter()
            .any(|item| item == "in position file"));
        assert!(lines[1].details.iter().any(|item| item == "LCSC C1"));
    }

    #[test]
    fn a_fiducial_with_a_pcbway_number_fails() {
        let mut mark = hole("FID1", "Fiducial:Fiducial_1mm_Mask2mm");
        mark.pcbway = Some("PW1".into());
        let lines = lines("attr", &[mark]);
        assert_eq!(lines[0].status, crate::ports::TestStatus::Failed);
        assert!(lines[0].details.iter().any(|item| item == "PCBWay PW1"));
    }

    #[test]
    fn no_marks_is_one_passing_line() {
        let lines = lines("attr", &[hole("R1", "Resistor_SMD:R_0805")]);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].status, crate::ports::TestStatus::Passed);
        assert!(!lines[0].subtest);
        assert!(lines[0].details[0].contains("no mounting holes"));
    }

    #[test]
    fn an_empty_footprint_is_not_a_mark() {
        assert!(!is_mounting_hole_or_fiducial(""));
        assert!(!is_mounting_hole_or_fiducial("Resistor_SMD:R_0805"));
        assert!(is_mounting_hole_or_fiducial(
            "MountingHole:MountingHole_3.2mm_M3"
        ));
    }
}
