//! One implementation per test type.

use anyhow::Result;
use serde_json::{json, Value};

use crate::ports::TestStatus;

use super::scheduler::TestContext;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    pub name: String,
    pub subtest: bool,
    pub status: TestStatus,
    pub details: Vec<String>,
    pub assertions: usize,
}

/// One line of a test file. `run` does the slow work when the scheduler asks.
pub struct Job {
    pub label: String,
    pub subtest: bool,
    pub run: Box<dyn FnOnce(&TestContext<'_>) -> Result<Line>>,
}

pub trait ValidatorInterface: Sync {
    fn type_name(&self) -> &'static str;
    fn schema(&self) -> Value;
    fn run(&self, file_name: &str, spec: &Value, ctx: &TestContext<'_>) -> Result<Vec<Job>>;
}

pub fn project_schema() -> Value {
    spec_schema(
        json!({
            "project": { "type": "string", "minLength": 1 }
        }),
        &["project"],
    )
}

pub fn spec_schema(properties: Value, required: &[&str]) -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": required,
        "properties": properties
    })
}

/// Wrap lines whose work is already finished.
pub fn ready_jobs(file_name: &str, lines: Vec<Line>) -> Vec<Job> {
    let expand = !(lines.len() == 1 && !lines[0].subtest);
    lines
        .into_iter()
        .map(|mut line| {
            if line.name.is_empty() {
                line.name = file_name.to_string();
            }
            line.subtest = expand && line.subtest;
            let label = line.name.clone();
            let subtest = line.subtest;
            Job {
                label,
                subtest,
                run: Box::new(move |_| Ok(line)),
            }
        })
        .collect()
}

pub fn validators() -> &'static [&'static dyn ValidatorInterface] {
    use super::attributes::Attributes;
    use super::availability::PartAvailability;
    use super::drc::DrcValidator;
    use super::electrical::{
        CatalogValue, Divider, ParallelPassives, ResistorPower, SeriesPassives, TimeConstant,
        VoltageRating,
    };
    use super::erc::ErcValidator;
    use super::jlcpcb_dfm::JLCPCBDFM;
    static ALL: &[&dyn ValidatorInterface] = &[
        &ErcValidator,
        &DrcValidator,
        &JLCPCBDFM,
        &Attributes,
        &PartAvailability,
        &CatalogValue,
        &ResistorPower,
        &VoltageRating,
        &Divider,
        &TimeConstant,
        &ParallelPassives,
        &SeriesPassives,
    ];
    ALL
}

pub fn find(type_name: &str) -> Option<&'static dyn ValidatorInterface> {
    validators()
        .iter()
        .copied()
        .find(|validator| validator.type_name() == type_name)
}
