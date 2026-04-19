//! CI output parser for automatic test failure intake.
//!
//! Parses `cargo test` and `cargo build` output into structured [`IntakeEvent`]s
//! that feed directly into the evolution intake pipeline.
//!
//! ## Supported formats
//!
//! - `cargo test` failure lines: `test <name> ... FAILED`
//! - `cargo build` errors: `error[E...]: <message> --> <file>:<line>`
//! - thread panics: `thread '<name>' panicked at '<msg>', <file>:<line>`
//! - Test result summary: `FAILED. 3 passed; 1 failed`
//!
//! ## Example
//!
//! ```rust
//! use oris_intake::ci_parser::CiParser;
//!
//! let output = "test my_module::test_foo ... FAILED\ntest result: FAILED. 0 passed; 1 failed";
//! let parser = CiParser::new();
//! let events = parser.parse(output);
//! assert!(!events.is_empty());
//! ```

use regex_lite::Regex;
use serde::{Deserialize, Serialize};

use crate::source::{IntakeEvent, IntakeSourceType, IssueSeverity};
use crate::{IntakeError, IntakeResult};

/// A single failure extracted from CI output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CiFailure {
    /// Type of failure detected.
    pub kind: CiFailureKind,
    /// Test or function name (if identifiable).
    pub name: Option<String>,
    /// Short description / error message.
    pub message: String,
    /// File location `file:line` (if available).
    pub location: Option<String>,
    /// Raw lines of context around the failure.
    pub context: String,
}

/// Categories of CI failures.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CiFailureKind {
    /// `cargo test` test case failed.
    TestFailure,
    /// `cargo build` / `cargo check` compiler error.
    CompilerError,
    /// Thread panic (`panicked at`).
    Panic,
    /// Generic build or run failure not matched above.
    GenericFailure,
}

impl std::fmt::Display for CiFailureKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CiFailureKind::TestFailure => write!(f, "test_failure"),
            CiFailureKind::CompilerError => write!(f, "compiler_error"),
            CiFailureKind::Panic => write!(f, "panic"),
            CiFailureKind::GenericFailure => write!(f, "generic_failure"),
        }
    }
}

/// Parses raw CI output (stdout/stderr) into [`CiFailure`] and [`IntakeEvent`] items.
pub struct CiParser {
    /// Regex for `test <name> ... FAILED`
    test_failed_re: Regex,
    /// Regex for `error[E...]: <msg>` (compiler error header)
    compiler_error_re: Regex,
    /// Regex for ` --> <file>:<line>:<col>` (compiler error location)
    location_re: Regex,
    /// Regex for `thread '<name>' panicked at`
    panic_re: Regex,
}

impl CiParser {
    /// Create a new parser with default patterns.
    pub fn new() -> Self {
        Self {
            test_failed_re: Regex::new(r"^test\s+(\S+)\s+\.\.\.\s+FAILED\s*$").unwrap(),
            compiler_error_re: Regex::new(r"^error(?:\[([A-Z]\d+)\])?:\s+(.+)$").unwrap(),
            location_re: Regex::new(r"^\s+-->\s+(.+:\d+(?::\d+)?)").unwrap(),
            panic_re: Regex::new(r"thread '([^']+)' panicked at '?([^,']+)'?").unwrap(),
        }
    }

    /// Parse raw CI output and return extracted failures.
    pub fn parse(&self, output: &str) -> Vec<CiFailure> {
        let mut failures = Vec::new();
        let lines: Vec<&str> = output.lines().collect();
        let mut i = 0;

        while i < lines.len() {
            let line = lines[i];

            // Pattern: test <name> ... FAILED
            if let Some(caps) = self.test_failed_re.captures(line) {
                let name = caps.get(1).map(|m| m.as_str().to_string());
                // Collect following context lines (assertion details, up to 10 lines)
                let context = self.collect_context(&lines, i + 1, 10);
                failures.push(CiFailure {
                    kind: CiFailureKind::TestFailure,
                    name: name.clone(),
                    message: format!(
                        "test {} failed",
                        name.as_deref().unwrap_or("unknown")
                    ),
                    location: None,
                    context,
                });
                i += 1;
                continue;
            }

            // Pattern: error[E...]: <msg>
            if let Some(caps) = self.compiler_error_re.captures(line) {
                let code = caps.get(1).map(|m| m.as_str().to_string());
                let msg = caps.get(2).map(|m| m.as_str()).unwrap_or("").to_string();

                // Look ahead for `-->` location
                let location = lines
                    .get(i + 1)
                    .and_then(|l| self.location_re.captures(l))
                    .and_then(|c| c.get(1))
                    .map(|m| m.as_str().to_string());

                let context = self.collect_context(&lines, i, 6);
                let message = if let Some(code) = &code {
                    format!("error[{}]: {}", code, msg)
                } else {
                    format!("error: {}", msg)
                };

                failures.push(CiFailure {
                    kind: CiFailureKind::CompilerError,
                    name: code,
                    message,
                    location,
                    context,
                });
                i += 1;
                continue;
            }

            // Pattern: thread '...' panicked at ...
            if let Some(caps) = self.panic_re.captures(line) {
                let thread = caps.get(1).map(|m| m.as_str()).unwrap_or("unknown");
                let msg = caps.get(2).map(|m| m.as_str()).unwrap_or("").trim();
                let context = self.collect_context(&lines, i, 8);
                failures.push(CiFailure {
                    kind: CiFailureKind::Panic,
                    name: Some(thread.to_string()),
                    message: format!("panic in '{}': {}", thread, msg),
                    location: None,
                    context,
                });
                i += 1;
                continue;
            }

            i += 1;
        }

        failures
    }

    /// Convert parsed failures into [`IntakeEvent`]s.
    pub fn to_intake_events(&self, failures: &[CiFailure]) -> Vec<IntakeEvent> {
        failures
            .iter()
            .map(|f| self.failure_to_event(f))
            .collect()
    }

    /// Parse raw output directly to [`IntakeEvent`]s.
    pub fn parse_to_events(&self, output: &str) -> Vec<IntakeEvent> {
        let failures = self.parse(output);
        self.to_intake_events(&failures)
    }

    fn failure_to_event(&self, failure: &CiFailure) -> IntakeEvent {
        let severity = match failure.kind {
            CiFailureKind::CompilerError => IssueSeverity::High,
            CiFailureKind::Panic => IssueSeverity::Critical,
            CiFailureKind::TestFailure => IssueSeverity::Medium,
            CiFailureKind::GenericFailure => IssueSeverity::Low,
        };

        let title = failure
            .name
            .as_deref()
            .map(|n| format!("[ci:{}] {}", failure.kind, n))
            .unwrap_or_else(|| format!("[ci:{}]", failure.kind));

        let description = if let Some(loc) = &failure.location {
            format!("{}\nat {}\n\n{}", failure.message, loc, failure.context)
        } else {
            format!("{}\n\n{}", failure.message, failure.context)
        };

        let signals = vec![
            failure.kind.to_string(),
            failure
                .name
                .as_deref()
                .unwrap_or("unknown")
                .to_string(),
        ];

        IntakeEvent {
            event_id: crate::generate_intake_id("ci"),
            source_type: IntakeSourceType::Github,
            source_event_id: None,
            title,
            description,
            severity,
            signals,
            raw_payload: None,
            timestamp_ms: chrono::Utc::now().timestamp_millis(),
        }
    }

    /// Collect up to `max_lines` of context starting from `start`, stopping at blank lines or next test.
    fn collect_context(&self, lines: &[&str], start: usize, max_lines: usize) -> String {
        lines
            .iter()
            .skip(start)
            .take(max_lines)
            .take_while(|l| !self.test_failed_re.is_match(l) && !l.trim().is_empty() || {
                // include one blank line as separator but stop at second
                l.trim().is_empty()
            })
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    }
}

impl Default for CiParser {
    fn default() -> Self {
        Self::new()
    }
}

/// Parse `cargo test` output and return [`IntakeEvent`]s ready for intake processing.
///
/// Convenience function wrapping [`CiParser::parse_to_events`].
pub fn parse_cargo_test_output(output: &str) -> Vec<IntakeEvent> {
    CiParser::new().parse_to_events(output)
}

/// Implement [`crate::source::IntakeSource`] for CI output strings via a wrapper.
pub struct CiIntakeSource {
    parser: CiParser,
}

impl CiIntakeSource {
    pub fn new() -> Self {
        Self {
            parser: CiParser::new(),
        }
    }
}

impl Default for CiIntakeSource {
    fn default() -> Self {
        Self::new()
    }
}

impl crate::source::IntakeSource for CiIntakeSource {
    fn source_type(&self) -> IntakeSourceType {
        IntakeSourceType::Github
    }

    fn process(&self, payload: &[u8]) -> IntakeResult<Vec<IntakeEvent>> {
        let text = std::str::from_utf8(payload)
            .map_err(|e| IntakeError::ParseError(e.to_string()))?;
        Ok(self.parser.parse_to_events(text))
    }

    fn validate(&self, payload: &[u8]) -> IntakeResult<()> {
        std::str::from_utf8(payload)
            .map(|_| ())
            .map_err(|e| IntakeError::ParseError(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_TEST_OUTPUT: &str = r#"running 3 tests
test kernel::test_replay_works ... ok
test kernel::test_snapshot_roundtrip ... FAILED
test evokernel::test_confidence_decay ... FAILED

failures:

---- kernel::test_snapshot_roundtrip stdout ----
thread 'kernel::test_snapshot_roundtrip' panicked at 'assertion `left == right` failed
  left: 42
 right: 0', crates/oris-kernel/src/kernel/snapshot.rs:88:9

---- evokernel::test_confidence_decay stdout ----
thread 'evokernel::test_confidence_decay' panicked at 'called `Result::unwrap()` on an `Err` value: ConfidenceError("decay factor out of range")', crates/oris-evokernel/src/core.rs:312:14

test result: FAILED. 1 passed; 2 failed; 0 ignored; 0 measured; 0 filtered out"#;

    const SAMPLE_COMPILER_OUTPUT: &str = r#"error[E0308]: mismatched types
 --> crates/oris-evolution/src/pipeline.rs:42:16
  |
42|     let x: u32 = "hello";
  |            ---   ^^^^^^^ expected `u32`, found `&str`

error[E0425]: cannot find value `missing_var` in this scope
 --> crates/oris-intake/src/ci_parser.rs:10:5"#;

    #[test]
    fn test_parse_test_failures() {
        let parser = CiParser::new();
        let failures = parser.parse(SAMPLE_TEST_OUTPUT);

        let test_failures: Vec<_> = failures
            .iter()
            .filter(|f| f.kind == CiFailureKind::TestFailure)
            .collect();

        assert_eq!(test_failures.len(), 2, "should detect 2 test failures");
        assert_eq!(
            test_failures[0].name.as_deref(),
            Some("kernel::test_snapshot_roundtrip")
        );
        assert_eq!(
            test_failures[1].name.as_deref(),
            Some("evokernel::test_confidence_decay")
        );
    }

    #[test]
    fn test_parse_panics() {
        let parser = CiParser::new();
        let failures = parser.parse(SAMPLE_TEST_OUTPUT);

        let panics: Vec<_> = failures
            .iter()
            .filter(|f| f.kind == CiFailureKind::Panic)
            .collect();

        assert!(!panics.is_empty(), "should detect panics from output");
    }

    #[test]
    fn test_parse_compiler_errors() {
        let parser = CiParser::new();
        let failures = parser.parse(SAMPLE_COMPILER_OUTPUT);

        let compiler_errors: Vec<_> = failures
            .iter()
            .filter(|f| f.kind == CiFailureKind::CompilerError)
            .collect();

        assert_eq!(compiler_errors.len(), 2);
        assert_eq!(compiler_errors[0].name.as_deref(), Some("E0308"));
        assert!(compiler_errors[0].message.contains("mismatched types"));
        assert!(
            compiler_errors[0].location.is_some(),
            "compiler error should have location"
        );
    }

    #[test]
    fn test_to_intake_events_severity() {
        let parser = CiParser::new();
        let failures = parser.parse(SAMPLE_COMPILER_OUTPUT);
        let events = parser.to_intake_events(&failures);

        assert!(!events.is_empty());
        for event in &events {
            // compiler errors → High severity
            assert_eq!(event.severity, IssueSeverity::High);
        }
    }

    #[test]
    fn test_parse_to_events_roundtrip() {
        let parser = CiParser::new();
        let events = parser.parse_to_events(SAMPLE_TEST_OUTPUT);

        assert!(!events.is_empty());
        for event in &events {
            assert!(!event.event_id.is_empty());
            assert!(event.event_id.starts_with("ci-"));
            assert!(!event.title.is_empty());
            assert!(!event.signals.is_empty());
            assert_eq!(event.source_type, IntakeSourceType::Github);
        }
    }

    #[test]
    fn test_empty_output_produces_no_events() {
        let parser = CiParser::new();
        let events = parser.parse_to_events("running 5 tests\ntest result: ok. 5 passed; 0 failed");
        assert!(events.is_empty(), "clean output should produce no intake events");
    }

    #[test]
    fn test_parse_cargo_test_output_convenience() {
        let events = parse_cargo_test_output(SAMPLE_TEST_OUTPUT);
        assert!(!events.is_empty());
    }

    #[test]
    fn test_ci_intake_source_process() {
        use crate::source::IntakeSource;

        let source = CiIntakeSource::new();
        let payload = SAMPLE_COMPILER_OUTPUT.as_bytes();

        source.validate(payload).expect("valid UTF-8 should pass validation");
        let events = source.process(payload).expect("processing should succeed");
        assert!(!events.is_empty());
    }

    #[test]
    fn test_ci_intake_source_invalid_utf8() {
        use crate::source::IntakeSource;

        let source = CiIntakeSource::new();
        let invalid = vec![0xFF, 0xFE, 0x00];
        assert!(source.validate(&invalid).is_err());
        assert!(source.process(&invalid).is_err());
    }
}
