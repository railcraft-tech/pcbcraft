//! PHPUnit-style test output.

use std::io::Write;
use std::time::Duration;

use pcbcraft_application::ports::{SuiteHeader, TestLine, TestPrinter, TestStatus};

pub struct ConsolePrinter<W> {
    out: W,
    color: bool,
}

impl<W: Write> ConsolePrinter<W> {
    pub fn new(out: W, color: bool) -> Self {
        Self { out, color }
    }

    fn paint(&self, code: &str, text: &str) -> String {
        if self.color {
            format!("\u{1b}[{code}m{text}\u{1b}[0m")
        } else {
            text.to_string()
        }
    }
}

impl<W: Write> ConsolePrinter<W> {
    fn emit(&mut self, line: &str) {
        let _ = writeln!(self.out, "{line}");
        let _ = self.out.flush();
    }
}

impl<W: Write> TestPrinter for ConsolePrinter<W> {
    fn schema_errors(&mut self, errors: &[String]) {
        for error in errors {
            let line = self.paint("31", error);
            self.emit(&line);
        }
    }

    fn suite(&mut self, header: &SuiteHeader) {
        let line = self.paint("1", &format!("{} ({})", header.name, header.count));
        self.emit(&line);
    }

    fn test_line(&mut self, line: &TestLine) {
        let mark = match line.status {
            TestStatus::Passed => self.paint("32", "✓"),
            TestStatus::Failed => self.paint("31", "✗"),
            TestStatus::Warning => self.paint("33", "!"),
            TestStatus::Skipped => self.paint("33", "s"),
        };
        let (head, tail) = if line.subtest {
            (line.group.clone(), format!(".{}", line.name))
        } else {
            (format!("{}:", line.group), format!(" {}", line.name))
        };
        let group = self.paint("35", &head);
        let name = self.paint("37", &tail);
        let time = if line.status == TestStatus::Skipped {
            self.paint("33", "(skipped)")
        } else {
            self.paint("90", &format!("({:.2}s)", line.duration.as_secs_f64()))
        };
        self.emit(&format!("{mark} {group}{name} {time}"));
        if line.status == TestStatus::Failed || line.status == TestStatus::Warning {
            let code = if line.status == TestStatus::Failed {
                "31"
            } else {
                "33"
            };
            for detail in &line.details {
                let line = self.paint(code, detail);
                self.emit(&format!("  {line}"));
            }
        }
    }

    fn separator(&mut self) {
        let line = self.paint("2", &"-".repeat(64));
        self.emit(&line);
    }

    fn summary(
        &mut self,
        elapsed: Duration,
        memory: &str,
        passed: bool,
        tests: usize,
        assertions: usize,
        skipped: usize,
        warnings: usize,
    ) {
        self.emit(&format!(
            "Time: {} ms, Memory: {memory}",
            elapsed.as_millis()
        ));
        let mut extras = Vec::new();
        if warnings > 0 {
            extras.push(format!("{warnings} warnings"));
        }
        if skipped > 0 {
            extras.push(format!("{skipped} skipped"));
        }
        let footer = if extras.is_empty() {
            format!("({tests} tests, {assertions} assertions)")
        } else {
            format!(
                "({tests} tests, {assertions} assertions, {})",
                extras.join(", ")
            )
        };
        if passed {
            let word = self.paint("32", "OK");
            self.emit(&format!("{word} {footer}"));
        } else {
            let word = self.paint("31", "FAILURES!");
            self.emit(&format!("{word} {footer}"));
        }
    }
}

pub fn memory_label() -> String {
    let Ok(text) = std::fs::read_to_string("/proc/self/status") else {
        return "n/a".into();
    };
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("VmHWM:") {
            let kb: f64 = rest
                .split_whitespace()
                .next()
                .and_then(|value| value.parse().ok())
                .unwrap_or(0.0);
            return format!("{:.2}MB", kb / 1024.0);
        }
    }
    "n/a".into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ok_footer_uses_the_phpunit_shape() {
        let mut buffer = Vec::new();
        let mut printer = ConsolePrinter::new(&mut buffer, false);
        printer.suite(&SuiteHeader {
            name: "ERCValidator".into(),
            count: 1,
        });
        printer.test_line(&TestLine {
            group: "ERCValidator".into(),
            name: "erc-test".into(),
            subtest: false,
            status: TestStatus::Passed,
            duration: Duration::from_millis(290),
            details: Vec::new(),
        });
        printer.summary(Duration::from_millis(904), "24.00MB", true, 1, 1, 0, 0);
        let text = String::from_utf8(buffer).unwrap();
        assert!(text.contains("ERCValidator (1)"));
        assert!(text.contains("✓ ERCValidator: erc-test (0.29s)"));
        assert!(text.contains("Time: 904 ms, Memory: 24.00MB"));
        assert!(text.contains("OK (1 tests, 1 assertions)"));
    }

    #[test]
    fn a_failure_prints_the_detail_and_the_failures_footer() {
        let mut buffer = Vec::new();
        let mut printer = ConsolePrinter::new(&mut buffer, false);
        printer.test_line(&TestLine {
            group: "DRCValidator".into(),
            name: "drc-test".into(),
            subtest: false,
            status: TestStatus::Failed,
            duration: Duration::from_millis(10),
            details: vec!["error: clearance".into()],
        });
        printer.summary(Duration::from_millis(10), "1.00MB", false, 1, 2, 0, 0);
        let text = String::from_utf8(buffer).unwrap();
        assert!(text.contains("✗ DRCValidator: drc-test"));
        assert!(text.contains("error: clearance"));
        assert!(text.contains("FAILURES! (1 tests, 2 assertions)"));
    }

    #[test]
    fn a_skip_is_counted_in_the_footer() {
        let mut buffer = Vec::new();
        let mut printer = ConsolePrinter::new(&mut buffer, false);
        printer.test_line(&TestLine {
            group: "VoltageRating".into(),
            name: "C21".into(),
            subtest: true,
            status: TestStatus::Skipped,
            duration: Duration::ZERO,
            details: Vec::new(),
        });
        printer.summary(Duration::from_millis(1), "1.00MB", true, 1, 0, 1, 0);
        let text = String::from_utf8(buffer).unwrap();
        assert!(text.contains("s VoltageRating.C21 (skipped)"));
        assert!(text.contains("OK (1 tests, 0 assertions, 1 skipped)"));
    }

    #[test]
    fn a_warning_is_printed_and_the_run_stays_ok() {
        let mut buffer = Vec::new();
        let mut printer = ConsolePrinter::new(&mut buffer, false);
        printer.test_line(&TestLine {
            group: "JLCPCBDFM".into(),
            name: "annular-ring".into(),
            subtest: true,
            status: TestStatus::Warning,
            duration: Duration::from_millis(4),
            details: vec!["0.150 mm is below the recommended 0.200 mm".into()],
        });
        printer.summary(Duration::from_millis(4), "1.00MB", true, 1, 1, 0, 1);
        let text = String::from_utf8(buffer).unwrap();
        assert!(text.contains("! JLCPCBDFM.annular-ring"));
        assert!(text.contains("0.150 mm is below the recommended 0.200 mm"));
        assert!(text.contains("OK (1 tests, 1 assertions, 1 warnings)"));
    }
}
