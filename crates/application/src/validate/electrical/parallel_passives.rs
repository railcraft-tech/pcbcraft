use anyhow::Result;
use serde_json::{json, Value};

use super::super::scheduler::TestContext;
use super::super::validator::{ready_jobs, spec_schema, Job, ValidatorInterface};
use super::{expect_object, parallel};

pub struct ParallelPassives;

impl ValidatorInterface for ParallelPassives {
    fn type_name(&self) -> &'static str {
        "ParallelPassives"
    }

    fn schema(&self) -> Value {
        spec_schema(
            json!({
                "project": { "type": "string", "minLength": 1 },
                "expect": expect_object()
            }),
            &["project"],
        )
    }

    fn run(&self, file_name: &str, spec: &Value, ctx: &TestContext<'_>) -> Result<Vec<Job>> {
        Ok(ready_jobs(
            file_name,
            parallel(file_name, spec, ctx.graph, ctx.cards)?,
        ))
    }
}
