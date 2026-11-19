use std::path::Path;

use anyhow::{Context, Result};
use pcbcraft_domain::validate::{Design, Report};
use serde::Deserialize;
use serde_json::Value;

use super::scheduler::TestContext;
use super::validator::{project_schema, Job, Line, ValidatorInterface};
use crate::ports::Fabrication;

pub struct ErcValidator;

#[derive(Deserialize)]
struct Spec {
    project: String,
}

impl ErcValidator {
    pub fn validate(&self, spec: &Value, fab: &dyn Fabrication) -> Result<Report> {
        let spec: Spec = serde_json::from_value(spec.clone()).context("spec ERC")?;
        let path = Path::new(&spec.project);
        let design = design_or_empty(fab, path)?;
        let violations = fab.erc(path)?;
        Ok(Report::from_design(design, violations))
    }
}

impl ValidatorInterface for ErcValidator {
    fn type_name(&self) -> &'static str {
        "ERCValidator"
    }

    fn schema(&self) -> Value {
        project_schema()
    }

    fn run(&self, file_name: &str, spec: &Value, ctx: &TestContext<'_>) -> Result<Vec<Job>> {
        let _ = ctx;
        let file_name = file_name.to_string();
        let spec = spec.clone();
        Ok(vec![Job {
            label: file_name.clone(),
            subtest: false,
            run: Box::new(move |ctx| {
                let report = ErcValidator.validate(&spec, ctx.fabrication)?;
                Ok(report_line(&file_name, &report))
            }),
        }])
    }
}

pub(crate) fn report_line(file_name: &str, report: &Report) -> Line {
    let details = report
        .violations
        .iter()
        .map(|item| {
            let severity = match item.severity {
                pcbcraft_domain::validate::Severity::Error => "error",
                pcbcraft_domain::validate::Severity::Warning => "warning",
                pcbcraft_domain::validate::Severity::Exclusion => "exclusion",
            };
            format!("{severity}: {}", item.description)
        })
        .collect();
    Line {
        name: file_name.to_string(),
        subtest: false,
        status: if report.has_error() {
            crate::ports::TestStatus::Failed
        } else {
            crate::ports::TestStatus::Passed
        },
        details,
        assertions: 1 + report.violations.len(),
    }
}

pub(crate) fn design_or_empty(fab: &dyn Fabrication, path: &Path) -> Result<Design> {
    match fab.design(path) {
        Ok(design) => Ok(design),
        Err(err) => Err(err),
    }
}
