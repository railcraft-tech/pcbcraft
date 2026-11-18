use anyhow::Result;
use serde_json::Value;

use super::super::scheduler::TestContext;
use super::super::validator::{ready_jobs, spec_schema, Job, ValidatorInterface};
use super::{divider, expect_schema, with_rails};

pub struct Divider;

impl ValidatorInterface for Divider {
    fn type_name(&self) -> &'static str {
        "Divider"
    }

    fn schema(&self) -> Value {
        spec_schema(with_rails(expect_schema()), &["project", "rails"])
    }

    fn run(&self, file_name: &str, spec: &Value, ctx: &TestContext<'_>) -> Result<Vec<Job>> {
        Ok(ready_jobs(
            file_name,
            divider(file_name, spec, ctx.graph, ctx.cards)?,
        ))
    }
}
