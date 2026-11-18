use anyhow::Result;
use serde_json::{json, Value};

use super::super::scheduler::TestContext;
use super::super::validator::{ready_jobs, spec_schema, Job, ValidatorInterface};
use super::catalog;

pub struct CatalogValue;

impl ValidatorInterface for CatalogValue {
    fn type_name(&self) -> &'static str {
        "CatalogValue"
    }

    fn schema(&self) -> Value {
        spec_schema(
            json!({ "project": { "type": "string", "minLength": 1 } }),
            &["project"],
        )
    }

    fn run(&self, file_name: &str, spec: &Value, ctx: &TestContext<'_>) -> Result<Vec<Job>> {
        Ok(ready_jobs(
            file_name,
            catalog(file_name, spec, ctx.graph, ctx.cards)?,
        ))
    }
}
