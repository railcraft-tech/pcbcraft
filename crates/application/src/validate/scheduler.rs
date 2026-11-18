//! Schemas first, then the run list, then the validators.

use std::time::{Duration, Instant};

use anyhow::Result;
use jsonschema::{Draft, Validator as JsonValidator};
use serde_json::Value;

use crate::ports::{
    Fabrication, NetGraphSource, PartCards, PartStock, SuiteHeader, TestLine, TestPrinter,
    TestStatus, UsedParts,
};

pub struct TestContext<'a> {
    pub fabrication: &'a dyn Fabrication,
    pub used_parts: &'a dyn UsedParts,
    pub stock: &'a dyn PartStock,
    pub graph: &'a dyn NetGraphSource,
    pub cards: &'a dyn PartCards,
}

#[derive(Debug, Clone)]
pub struct LoadedTest {
    pub name: String,
    pub type_name: String,
    pub spec: Value,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScheduleFailure {
    pub errors: Vec<String>,
}

pub struct FinishedTest {
    pub line: TestLine,
    pub assertions: usize,
    pub failed: bool,
}

pub fn check_schemas(tests: &[LoadedTest]) -> Result<(), ScheduleFailure> {
    let mut errors = Vec::new();
    for test in tests {
        let schema = match schema_for(&test.type_name) {
            Some(schema) => schema,
            None => {
                errors.push(format!(
                    "{}: unknown validator type '{}'",
                    test.name, test.type_name
                ));
                continue;
            }
        };
        let compiled = match JsonValidator::options()
            .with_draft(Draft::Draft7)
            .build(&schema)
        {
            Ok(compiled) => compiled,
            Err(err) => {
                errors.push(format!("{}: schema: {err}", test.name));
                continue;
            }
        };
        if let Err(error) = compiled.validate(&test.spec) {
            errors.push(format!("{}: {error}", test.name));
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(ScheduleFailure { errors })
    }
}

pub fn order(mut tests: Vec<LoadedTest>) -> Vec<LoadedTest> {
    tests.sort_by(|left, right| {
        type_rank(&left.type_name)
            .cmp(&type_rank(&right.type_name))
            .then_with(|| left.name.cmp(&right.name))
    });
    tests
}

pub fn known_types() -> &'static [&'static str] {
    KNOWN_TYPES
}

const KNOWN_TYPES: &[&str] = &[
    "ERCValidator",
    "DRCValidator",
    "Attributes",
    "PartAvailability",
    "CatalogValue",
    "ResistorPower",
    "VoltageRating",
    "Divider",
    "TimeConstant",
    "ParallelPassives",
    "SeriesPassives",
];

fn type_rank(type_name: &str) -> u8 {
    KNOWN_TYPES
        .iter()
        .position(|name| *name == type_name)
        .map(|index| index as u8)
        .unwrap_or(19)
}

fn schema_for(type_name: &str) -> Option<Value> {
    super::validator::find(type_name).map(|validator| validator.schema())
}

pub fn run_tests(
    tests: Vec<LoadedTest>,
    ctx: &TestContext<'_>,
    printer: &mut dyn TestPrinter,
) -> Result<RunTotals> {
    run_tests_skipping(tests, ctx, printer, &[])
}

pub fn run_tests_skipping(
    tests: Vec<LoadedTest>,
    ctx: &TestContext<'_>,
    printer: &mut dyn TestPrinter,
    skipped: &[String],
) -> Result<RunTotals> {
    if let Err(failure) = check_schemas(&tests) {
        printer.schema_errors(&failure.errors);
        return Ok(RunTotals {
            blocked: true,
            ..RunTotals::default()
        });
    }
    let tests = order(tests);
    let started = Instant::now();
    let mut totals = RunTotals::default();
    let mut skips = super::skip::Skips::new(skipped.iter().cloned());
    let mut index = 0;
    while index < tests.len() {
        let group = tests[index].type_name.clone();
        let end = tests[index..]
            .iter()
            .position(|test| test.type_name != group)
            .map(|offset| index + offset)
            .unwrap_or(tests.len());
        if index > 0 {
            printer.separator();
        }
        let validator = super::validator::find(&group).expect("schema checked");
        let mut jobs = Vec::new();
        for test in &tests[index..end] {
            match validator.run(&test.name, &test.spec, ctx) {
                Ok(batch) => jobs.extend(batch),
                Err(err) => jobs.push(failed_job(&test.name, err.to_string())),
            }
        }
        printer.suite(&SuiteHeader {
            name: group.clone(),
            count: jobs.len(),
        });
        for job in jobs {
            let label = job.label.clone();
            let subtest = job.subtest;
            if skips.would_match(&group, &label, subtest) {
                skips.mark(&group, &label, subtest);
                record(
                    skipped_line(&group, &label, subtest),
                    printer,
                    &mut totals,
                    &mut skips,
                );
                continue;
            }
            let started = Instant::now();
            match (job.run)(ctx) {
                Ok(line) => {
                    let mut item = finished_line(&group, line);
                    item.line.duration = started.elapsed();
                    record(item, printer, &mut totals, &mut skips);
                }
                Err(err) => record(
                    finished_line(&group, failed_line(&label, subtest, err.to_string())),
                    printer,
                    &mut totals,
                    &mut skips,
                ),
            }
        }
        index = end;
    }
    totals.elapsed = started.elapsed();
    totals.unmatched_skips = skips.unmatched();
    Ok(totals)
}

fn record(
    mut item: FinishedTest,
    printer: &mut dyn TestPrinter,
    totals: &mut RunTotals,
    skips: &mut super::skip::Skips,
) {
    if item.line.status != TestStatus::Skipped
        && skips.mark(&item.line.group, &item.line.name, item.line.subtest)
    {
        item.line.name = stable_label(&item.line.name, item.line.subtest);
        item.line.status = TestStatus::Skipped;
        item.line.details.clear();
        item.failed = false;
        item.assertions = 0;
    }
    totals.tests += 1;
    if item.line.status == TestStatus::Skipped {
        totals.skipped += 1;
    } else {
        totals.assertions += item.assertions;
        if item.failed {
            totals.failed += 1;
        }
    }
    printer.test_line(&item.line);
}

fn stable_label(name: &str, subtest: bool) -> String {
    if subtest {
        name.split_whitespace().next().unwrap_or(name).to_string()
    } else {
        name.to_string()
    }
}

fn failed_job(name: &str, detail: String) -> super::validator::Job {
    let name = name.to_string();
    super::validator::Job {
        label: name.clone(),
        subtest: false,
        run: Box::new(move |_| Ok(failed_line(&name, false, detail.clone()))),
    }
}

fn failed_line(name: &str, subtest: bool, detail: String) -> super::validator::Line {
    super::validator::Line {
        name: name.to_string(),
        subtest,
        passed: false,
        details: vec![detail],
        assertions: 1,
    }
}

fn finished_line(group: &str, line: super::validator::Line) -> FinishedTest {
    let passed = line.passed;
    FinishedTest {
        line: TestLine {
            group: group.to_string(),
            name: line.name,
            subtest: line.subtest,
            status: if passed {
                TestStatus::Passed
            } else {
                TestStatus::Failed
            },
            duration: Duration::ZERO,
            details: line.details,
        },
        assertions: line.assertions,
        failed: !passed,
    }
}

fn skipped_line(group: &str, name: &str, subtest: bool) -> FinishedTest {
    FinishedTest {
        line: TestLine {
            group: group.to_string(),
            name: name.to_string(),
            subtest,
            status: TestStatus::Skipped,
            duration: Duration::ZERO,
            details: Vec::new(),
        },
        assertions: 0,
        failed: false,
    }
}

#[derive(Debug, Clone)]
pub struct RunTotals {
    pub blocked: bool,
    pub tests: usize,
    pub failed: usize,
    pub assertions: usize,
    pub skipped: usize,
    pub unmatched_skips: Vec<String>,
    pub elapsed: Duration,
}

impl Default for RunTotals {
    fn default() -> Self {
        Self {
            blocked: false,
            tests: 0,
            failed: 0,
            assertions: 0,
            skipped: 0,
            unmatched_skips: Vec::new(),
            elapsed: Duration::ZERO,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pcbcraft_domain::drawing::Side;
    use pcbcraft_domain::net::NetGraph;
    use pcbcraft_domain::parts::{PartRecord, Supplier};
    use pcbcraft_domain::validate::{Design, Violation};
    use std::cell::RefCell;
    use std::path::Path;
    use std::rc::Rc;

    struct NoParts;
    impl UsedParts for NoParts {
        fn list(&self, _: &Path) -> Result<Vec<crate::ports::UsedPart>> {
            Ok(Vec::new())
        }
    }
    struct NoStock;
    impl PartStock for NoStock {
        fn lookup(&self, _: pcbcraft_domain::parts::Supplier, _: &str) -> Result<Option<u64>> {
            Ok(None)
        }
    }

    fn context<'a>(
        fab: &'a FakeFab,
        graph: &'a EmptyGraph,
        cards: &'a EmptyCards,
    ) -> TestContext<'a> {
        const PARTS: NoParts = NoParts;
        const STOCK: NoStock = NoStock;
        TestContext {
            fabrication: fab,
            used_parts: &PARTS,
            stock: &STOCK,
            graph,
            cards,
        }
    }

    struct EmptyGraph;
    impl NetGraphSource for EmptyGraph {
        fn load(&self, _: &Path) -> Result<NetGraph> {
            Ok(NetGraph::default())
        }
    }
    struct EmptyCards;
    impl PartCards for EmptyCards {
        fn card(&self, _: &Path, _: Supplier, _: &str) -> Result<Option<PartRecord>> {
            Ok(None)
        }
    }

    struct FakeFab {
        log: Rc<RefCell<Vec<String>>>,
    }

    impl FakeFab {
        fn quiet() -> Self {
            Self {
                log: Rc::new(RefCell::new(Vec::new())),
            }
        }
    }

    impl Fabrication for FakeFab {
        fn export_bom(&self, _: &Path, _: &Path) -> Result<()> {
            Ok(())
        }
        fn export_pos(&self, _: &Path, _: &Path) -> Result<()> {
            Ok(())
        }
        fn export_gerbers(&self, _: &Path, _: &Path) -> Result<()> {
            Ok(())
        }
        fn export_drill_split(&self, _: &Path, _: &Path) -> Result<()> {
            Ok(())
        }
        fn export_dxf_outline(&self, _: &Path, _: &Path) -> Result<()> {
            Ok(())
        }
        fn export_excellon(&self, _: &Path, _: &Path) -> Result<()> {
            Ok(())
        }
        fn export_svg(&self, _: &Path, _: Side, _: bool, _: &Path) -> Result<()> {
            Ok(())
        }
        fn erc(&self, _: &Path) -> Result<Vec<Violation>> {
            self.log.borrow_mut().push("erc".into());
            Ok(Vec::new())
        }
        fn drc(&self, _: &Path) -> Result<Vec<Violation>> {
            self.log.borrow_mut().push("drc".into());
            Ok(Vec::new())
        }
        fn design(&self, _: &Path) -> Result<Design> {
            Ok(Design::default())
        }
    }

    struct MemoryPrinter {
        groups: Vec<String>,
        blocked: bool,
    }

    impl TestPrinter for MemoryPrinter {
        fn schema_errors(&mut self, _: &[String]) {
            self.blocked = true;
        }
        fn suite(&mut self, header: &SuiteHeader) {
            self.groups.push(header.name.clone());
        }
        fn test_line(&mut self, _: &TestLine) {}
        fn separator(&mut self) {}
        fn summary(&mut self, _: Duration, _: &str, _: bool, _: usize, _: usize, _: usize) {}
    }

    #[test]
    fn every_known_type_has_a_schema() {
        let registered: Vec<_> = super::super::validator::validators()
            .iter()
            .map(|validator| validator.type_name())
            .collect();
        assert_eq!(registered, known_types());
        for type_name in known_types() {
            assert!(schema_for(type_name).is_some(), "{type_name}");
        }
    }

    #[test]
    fn a_bad_schema_does_not_run() {
        let tests = vec![LoadedTest {
            name: "erc-test".into(),
            type_name: "ERCValidator".into(),
            spec: serde_json::json!({}),
        }];
        let mut printer = MemoryPrinter {
            groups: Vec::new(),
            blocked: false,
        };
        let fab = FakeFab::quiet();
        let totals = run_tests(
            tests,
            &context(&fab, &EmptyGraph, &EmptyCards),
            &mut printer,
        )
        .unwrap();
        assert!(totals.blocked);
        assert!(printer.blocked);
        assert!(printer.groups.is_empty());
    }

    #[test]
    fn groups_run_erc_before_drc() {
        let tests = vec![
            LoadedTest {
                name: "b".into(),
                type_name: "DRCValidator".into(),
                spec: serde_json::json!({"project": "b.kicad_pro"}),
            },
            LoadedTest {
                name: "a".into(),
                type_name: "ERCValidator".into(),
                spec: serde_json::json!({"project": "a.kicad_pro"}),
            },
        ];
        let mut printer = MemoryPrinter {
            groups: Vec::new(),
            blocked: false,
        };
        let fab = FakeFab::quiet();
        run_tests(
            tests,
            &context(&fab, &EmptyGraph, &EmptyCards),
            &mut printer,
        )
        .unwrap();
        assert_eq!(printer.groups, vec!["ERCValidator", "DRCValidator"]);
    }

    struct TracePrinter {
        log: Rc<RefCell<Vec<String>>>,
    }

    impl TestPrinter for TracePrinter {
        fn schema_errors(&mut self, _: &[String]) {}
        fn suite(&mut self, header: &SuiteHeader) {
            self.log
                .borrow_mut()
                .push(format!("suite {} {}", header.name, header.count));
        }
        fn test_line(&mut self, line: &TestLine) {
            let kind = if line.subtest { "sub" } else { "test" };
            self.log.borrow_mut().push(format!("{kind} {}", line.name));
        }
        fn separator(&mut self) {}
        fn summary(&mut self, _: Duration, _: &str, _: bool, _: usize, _: usize, _: usize) {}
    }

    struct Listing {
        log: Rc<RefCell<Vec<String>>>,
    }

    impl UsedParts for Listing {
        fn list(&self, _: &Path) -> Result<Vec<crate::ports::UsedPart>> {
            self.log.borrow_mut().push("list".into());
            Ok(vec![
                crate::ports::UsedPart {
                    supplier: Supplier::Lcsc,
                    part_id: "C2".into(),
                },
                crate::ports::UsedPart {
                    supplier: Supplier::Lcsc,
                    part_id: "C1".into(),
                },
            ])
        }
    }

    struct LoggingStock {
        log: Rc<RefCell<Vec<String>>>,
    }

    impl PartStock for LoggingStock {
        fn lookup(&self, _: Supplier, part_id: &str) -> Result<Option<u64>> {
            self.log.borrow_mut().push(format!("stock {part_id}"));
            Ok(Some(100))
        }
    }

    #[test]
    fn each_line_is_printed_before_the_next_check() {
        let log = Rc::new(RefCell::new(Vec::new()));
        let fab = FakeFab { log: log.clone() };
        let parts = Listing { log: log.clone() };
        let stock = LoggingStock { log: log.clone() };
        let ctx = TestContext {
            fabrication: &fab,
            used_parts: &parts,
            stock: &stock,
            graph: &EmptyGraph,
            cards: &EmptyCards,
        };
        let tests = vec![
            LoadedTest {
                name: "d".into(),
                type_name: "DRCValidator".into(),
                spec: serde_json::json!({"project": "d.kicad_pro"}),
            },
            LoadedTest {
                name: "e".into(),
                type_name: "ERCValidator".into(),
                spec: serde_json::json!({"project": "e.kicad_pro"}),
            },
            LoadedTest {
                name: "stock".into(),
                type_name: "PartAvailability".into(),
                spec: serde_json::json!({"project": "p.kicad_pro"}),
            },
        ];
        let mut printer = TracePrinter { log: log.clone() };
        run_tests(tests, &ctx, &mut printer).unwrap();
        assert_eq!(
            log.borrow().as_slice(),
            [
                "suite ERCValidator 1",
                "erc",
                "test e",
                "suite DRCValidator 1",
                "drc",
                "test d",
                "list",
                "suite PartAvailability 2",
                "stock C1",
                "sub C1",
                "stock C2",
                "sub C2",
            ]
        );
    }

    struct OnePart;
    impl UsedParts for OnePart {
        fn list(&self, _: &Path) -> Result<Vec<crate::ports::UsedPart>> {
            Ok(vec![crate::ports::UsedPart {
                supplier: Supplier::Lcsc,
                part_id: "C21".into(),
            }])
        }
    }

    struct BoomStock;
    impl PartStock for BoomStock {
        fn lookup(&self, _: Supplier, _: &str) -> Result<Option<u64>> {
            panic!("stock lookup");
        }
    }

    struct SkipPrinter {
        skipped: Vec<String>,
    }

    impl TestPrinter for SkipPrinter {
        fn schema_errors(&mut self, _: &[String]) {}
        fn suite(&mut self, _: &SuiteHeader) {}
        fn test_line(&mut self, line: &TestLine) {
            if line.status == TestStatus::Skipped {
                self.skipped.push(format!("{}.{}", line.group, line.name));
            }
        }
        fn separator(&mut self) {}
        fn summary(&mut self, _: Duration, _: &str, _: bool, _: usize, _: usize, _: usize) {}
    }

    #[test]
    fn a_skipped_part_does_not_ask_for_stock() {
        let fab = FakeFab::quiet();
        let parts = OnePart;
        let stock = BoomStock;
        let ctx = TestContext {
            fabrication: &fab,
            used_parts: &parts,
            stock: &stock,
            graph: &EmptyGraph,
            cards: &EmptyCards,
        };
        let tests = vec![LoadedTest {
            name: "availability-test".into(),
            type_name: "PartAvailability".into(),
            spec: serde_json::json!({"project": "board.kicad_pro"}),
        }];
        let mut printer = SkipPrinter {
            skipped: Vec::new(),
        };
        let totals = run_tests_skipping(
            tests,
            &ctx,
            &mut printer,
            &["PartAvailability.C21".into(), "Divider.R9".into()],
        )
        .unwrap();
        assert_eq!(totals.failed, 0);
        assert_eq!(totals.skipped, 1);
        assert_eq!(totals.assertions, 0);
        assert_eq!(printer.skipped, vec!["PartAvailability.C21".to_string()]);
        assert_eq!(totals.unmatched_skips, vec!["Divider.R9".to_string()]);
    }
}
