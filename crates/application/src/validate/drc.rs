use std::path::Path;

use anyhow::{Context, Result};
use pcbcraft_domain::validate::Report;
use serde::Deserialize;
use serde_json::Value;

use super::erc::design_or_empty;
use super::scheduler::TestContext;
use super::validator::{project_schema, Job, ValidatorInterface};
use crate::ports::Fabrication;

pub struct DrcValidator;

#[derive(Deserialize)]
struct Spec {
    project: String,
}

impl DrcValidator {
    pub fn validate(&self, spec: &Value, fab: &dyn Fabrication) -> Result<Report> {
        let spec: Spec = serde_json::from_value(spec.clone()).context("spec DRC")?;
        let path = Path::new(&spec.project);
        let design = design_or_empty(fab, path)?;
        let violations = fab.drc(path)?;
        Ok(Report::from_design(design, violations))
    }
}

impl ValidatorInterface for DrcValidator {
    fn type_name(&self) -> &'static str {
        "DRCValidator"
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
                let report = DrcValidator.validate(&spec, ctx.fabrication)?;
                Ok(super::erc::report_line(&file_name, &report))
            }),
        }])
    }
}
