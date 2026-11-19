use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TestStatus {
    Passed,
    Failed,
    Warning,
    Skipped,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SuiteHeader {
    pub name: String,
    pub count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestLine {
    pub group: String,
    pub name: String,
    /// When set, the printed name is `{group}.{name}` instead of `{group}: {name}`.
    pub subtest: bool,
    pub status: TestStatus,
    pub duration: Duration,
    pub details: Vec<String>,
}

pub trait TestPrinter {
    fn schema_errors(&mut self, errors: &[String]);
    fn suite(&mut self, header: &SuiteHeader);
    fn test_line(&mut self, line: &TestLine);
    fn separator(&mut self);
    fn summary(
        &mut self,
        elapsed: Duration,
        memory: &str,
        passed: bool,
        tests: usize,
        assertions: usize,
        skipped: usize,
        warnings: usize,
    );
}
