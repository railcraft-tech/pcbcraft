use anyhow::Result;
use serde_json::{json, Value};

use super::super::scheduler::TestContext;
use super::super::validator::{ready_jobs, spec_schema, Job, ValidatorInterface};
use super::{power, with_rails};

pub struct ResistorPower;

impl ValidatorInterface for ResistorPower {
    fn type_name(&self) -> &'static str {
        "ResistorPower"
    }

    fn schema(&self) -> Value {
        spec_schema(
            with_rails(json!({ "powerHeadroom": { "type": "number", "minimum": 1 } })),
            &["project", "rails"],
        )
    }

    fn run(&self, file_name: &str, spec: &Value, ctx: &TestContext<'_>) -> Result<Vec<Job>> {
        Ok(ready_jobs(
            file_name,
            power(file_name, spec, ctx.graph, ctx.cards)?,
        ))
    }
}
