//! Continuous autonomous intake sources.
//!
//! This module provides the `ContinuousIntakeSource` trait and concrete
//! implementations for CI failures, test regressions, lint/compile regressions,
//! and runtime panics.  Each implementation normalises raw diagnostic text into
//! an [`AutonomousIntakeInput`] ready to be fed to
//! `EvoKernel::discover_autonomous_candidates()`.
//!
//! # Architecture
//!
//! ```text
//! Raw diagnostic text / log lines
//!         |
//!         v
//! ContinuousIntakeSource::extract(lines) -> AutonomousIntakeInput
//!         |
//!         v
//! EvoKernel::discover_autonomous_candidates(input) -> AutonomousIntakeOutput
//! ```
//!
//! No mutation, no proposal, and no execution happen here.  This layer is
//! strictly discovery and classification.

use oris_agent_contract::{AutonomousCandidateSource, AutonomousIntakeInput};

// ── Trait ─────────────────────────────────────────────────────────────────────

/// A source that can extract an [`AutonomousIntakeInput`] from raw diagnostic
/// lines without any caller-supplied issue metadata.
///
/// Implementors are expected to:
/// - Filter and normalise relevant lines from the raw input.
/// - Return a stable `source_id` based on the raw content so that duplicate
///   runs collapse to the same identity.
/// - Emit an [`AutonomousIntakeInput`] even when no relevant signals are found
///   (with an empty `raw_signals` vec); the kernel treats this as an immediate
///   fail-closed unsupported input.
pub trait ContinuousIntakeSource: Send + Sync {
    /// Human-readable name of this source (for logging / metrics).
    fn name(&self) -> &'static str;

    /// The [`AutonomousCandidateSource`] variant this implementation covers.
    fn candidate_source(&self) -> AutonomousCandidateSource;

    /// Extract a normalized [`AutonomousIntakeInput`] from `raw_lines`.
    ///
    /// `run_identifier` is an opaque string supplied by the caller (e.g. a CI
    /// run ID, log stream name, or timestamp) used to form the `source_id`.
    /// When `None`, the implementation must derive a stable identifier from
    /// the content itself.
    fn extract(&self, raw_lines: &[String], run_identifier: Option<&str>) -> AutonomousIntakeInput;
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Derive a stable source_id from the raw lines when no explicit run
/// identifier is available.
fn content_derived_source_id(prefix: &str, lines: &[String]) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    for l in lines {
        l.hash(&mut hasher);
    }
    format!("{prefix}:{:016x}", hasher.finish())
}

fn make_source_id(prefix: &str, lines: &[String], run_id: Option<&str>) -> String {
    match run_id {
        Some(id) if !id.is_empty() => format!("{prefix}:{id}"),
        _ => content_derived_source_id(prefix, lines),
    }
}

// ── CiFailureSource ───────────────────────────────────────────────────────────

/// Extracts autonomous intake signals from CI failure logs (GitHub Actions,
/// GitLab CI, or any CI system that emits build/test output to stdout).
///
/// Relevant lines are those that contain Rust compiler errors (`error[E…]`),
/// `FAILED`, `error:`, or `panicked at`.
#[derive(Clone, Debug, Default)]
pub struct CiFailureSource;

impl ContinuousIntakeSource for CiFailureSource {
    fn name(&self) -> &'static str {
        "ci_failure"
    }

    fn candidate_source(&self) -> AutonomousCandidateSource {
        AutonomousCandidateSource::CiFailure
    }

    fn extract(&self, raw_lines: &[String], run_id: Option<&str>) -> AutonomousIntakeInput {
        let relevant: Vec<String> = raw_lines
            .iter()
            .filter(|l| {
                let lo = l.to_ascii_lowercase();
                lo.contains("error[e") || lo.contains("error:") || lo.contains("failed")
            })
            .cloned()
            .collect();
        AutonomousIntakeInput {
            source_id: make_source_id("ci-failure", raw_lines, run_id),
            candidate_source: AutonomousCandidateSource::CiFailure,
            raw_signals: relevant,
        }
    }
}

// ── TestRegressionSource ──────────────────────────────────────────────────────

/// Extracts autonomous intake signals from `cargo test` output.
///
/// Relevant lines are those that contain `FAILED`, `panicked at`, or
/// `test … … FAILED`.
#[derive(Clone, Debug, Default)]
pub struct TestRegressionSource;

impl ContinuousIntakeSource for TestRegressionSource {
    fn name(&self) -> &'static str {
        "test_regression"
    }

    fn candidate_source(&self) -> AutonomousCandidateSource {
        AutonomousCandidateSource::TestRegression
    }

    fn extract(&self, raw_lines: &[String], run_id: Option<&str>) -> AutonomousIntakeInput {
        let relevant: Vec<String> = raw_lines
            .iter()
            .filter(|l| {
                let lo = l.to_ascii_lowercase();
                lo.contains("failed") || lo.contains("panicked at") || lo.contains("test result")
            })
            .cloned()
            .collect();
        AutonomousIntakeInput {
            source_id: make_source_id("test-regression", raw_lines, run_id),
            candidate_source: AutonomousCandidateSource::TestRegression,
            raw_signals: relevant,
        }
    }
}

// ── LintRegressionSource ──────────────────────────────────────────────────────

/// Extracts autonomous intake signals from `cargo clippy` output.
///
/// Relevant lines contain `warning:`, `error:`, or `help:` annotations.
#[derive(Clone, Debug, Default)]
pub struct LintRegressionSource;

impl ContinuousIntakeSource for LintRegressionSource {
    fn name(&self) -> &'static str {
        "lint_regression"
    }

    fn candidate_source(&self) -> AutonomousCandidateSource {
        AutonomousCandidateSource::LintRegression
    }

    fn extract(&self, raw_lines: &[String], run_id: Option<&str>) -> AutonomousIntakeInput {
        let relevant: Vec<String> = raw_lines
            .iter()
            .filter(|l| {
                let lo = l.to_ascii_lowercase();
                lo.contains("warning:") || lo.contains("error:") || lo.contains("help:")
            })
            .cloned()
            .collect();
        AutonomousIntakeInput {
            source_id: make_source_id("lint-regression", raw_lines, run_id),
            candidate_source: AutonomousCandidateSource::LintRegression,
            raw_signals: relevant,
        }
    }
}

// ── CompileRegressionSource ───────────────────────────────────────────────────

/// Extracts autonomous intake signals from `cargo build` or `rustc` output.
///
/// Relevant lines contain `error[E…]` compiler error codes.
#[derive(Clone, Debug, Default)]
pub struct CompileRegressionSource;

impl ContinuousIntakeSource for CompileRegressionSource {
    fn name(&self) -> &'static str {
        "compile_regression"
    }

    fn candidate_source(&self) -> AutonomousCandidateSource {
        AutonomousCandidateSource::CompileRegression
    }

    fn extract(&self, raw_lines: &[String], run_id: Option<&str>) -> AutonomousIntakeInput {
        let relevant: Vec<String> = raw_lines
            .iter()
            .filter(|l| {
                let lo = l.to_ascii_lowercase();
                // Rust compiler errors have the form `error[Exxxxx]`
                lo.contains("error[e") || lo.contains("aborting due to")
            })
            .cloned()
            .collect();
        AutonomousIntakeInput {
            source_id: make_source_id("compile-regression", raw_lines, run_id),
            candidate_source: AutonomousCandidateSource::CompileRegression,
            raw_signals: relevant,
        }
    }
}

// ── RuntimePanicSource ────────────────────────────────────────────────────────

/// Extracts autonomous intake signals from runtime panic output.
///
/// Relevant lines contain `panicked at`, `thread '…' panicked`, or
/// `SIGSEGV` / `SIGABRT` indicators.
///
/// Note: `AutonomousCandidateSource::RuntimeIncident` is currently **not**
/// mapped to a `BoundedTaskClass` by the kernel classifier, so intake from
/// this source will return a fail-closed `UnsupportedSignalClass` candidate.
/// This behaviour is intentional and reflects the current autonomy boundary.
#[derive(Clone, Debug, Default)]
pub struct RuntimePanicSource;

impl ContinuousIntakeSource for RuntimePanicSource {
    fn name(&self) -> &'static str {
        "runtime_panic"
    }

    fn candidate_source(&self) -> AutonomousCandidateSource {
        AutonomousCandidateSource::RuntimeIncident
    }

    fn extract(&self, raw_lines: &[String], run_id: Option<&str>) -> AutonomousIntakeInput {
        let relevant: Vec<String> = raw_lines
            .iter()
            .filter(|l| {
                let lo = l.to_ascii_lowercase();
                lo.contains("panicked at")
                    || lo.contains("thread '")
                    || lo.contains("sigsegv")
                    || lo.contains("sigabrt")
            })
            .cloned()
            .collect();
        AutonomousIntakeInput {
            source_id: make_source_id("runtime-panic", raw_lines, run_id),
            candidate_source: AutonomousCandidateSource::RuntimeIncident,
            raw_signals: relevant,
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(raw: &[&str]) -> Vec<String> {
        raw.iter().map(|s| s.to_string()).collect()
    }

    // ── CiFailureSource ───────────────────────────────────────────────────

    #[test]
    fn continuous_intake_ci_failure_extracts_error_lines() {
        let source = CiFailureSource;
        let raw = lines(&[
            "running 12 tests",
            "error[E0382]: borrow of moved value: `x`",
            "test foo ... FAILED",
            "test bar ... ok",
        ]);
        let input = source.extract(&raw, Some("run-42"));
        assert_eq!(input.candidate_source, AutonomousCandidateSource::CiFailure);
        assert_eq!(input.source_id, "ci-failure:run-42");
        assert!(!input.raw_signals.is_empty());
        assert!(input.raw_signals.iter().any(|s| s.contains("error[E0382]")));
    }

    #[test]
    fn continuous_intake_ci_failure_empty_on_clean_output() {
        let source = CiFailureSource;
        let raw = lines(&["running 3 tests", "test a ... ok", "3 tests passed"]);
        let input = source.extract(&raw, Some("run-clean"));
        // No error lines → empty raw_signals; kernel will fail-closed on empty input
        assert!(input.raw_signals.is_empty());
    }

    #[test]
    fn continuous_intake_ci_failure_stable_source_id_without_run_id() {
        let source = CiFailureSource;
        let raw = lines(&["error: something went wrong"]);
        let id1 = source.extract(&raw, None).source_id;
        let id2 = source.extract(&raw, None).source_id;
        assert_eq!(
            id1, id2,
            "source_id must be deterministic for the same content"
        );
    }

    // ── TestRegressionSource ──────────────────────────────────────────────

    #[test]
    fn continuous_intake_test_regression_captures_failed_lines() {
        let source = TestRegressionSource;
        let raw = lines(&[
            "test tests::my_test ... FAILED",
            "thread 'main' panicked at 'assertion failed'",
            "test other::test ... ok",
        ]);
        let input = source.extract(&raw, Some("push-abc123"));
        assert_eq!(
            input.candidate_source,
            AutonomousCandidateSource::TestRegression
        );
        assert!(!input.raw_signals.is_empty());
    }

    #[test]
    fn continuous_intake_test_regression_dedup_key_stable() {
        let source = TestRegressionSource;
        let raw = lines(&["FAILED: test_foo"]);
        let a = source.extract(&raw, None);
        let b = source.extract(&raw, None);
        assert_eq!(a.source_id, b.source_id);
    }

    // ── LintRegressionSource ──────────────────────────────────────────────

    #[test]
    fn continuous_intake_lint_regression_captures_warnings() {
        let source = LintRegressionSource;
        let raw = lines(&[
            "warning: unused variable `x`",
            "  --> src/lib.rs:10:5",
            "error: unused import",
        ]);
        let input = source.extract(&raw, Some("lint-01"));
        assert_eq!(
            input.candidate_source,
            AutonomousCandidateSource::LintRegression
        );
        assert!(input.raw_signals.len() >= 2);
    }

    #[test]
    fn continuous_intake_lint_regression_empty_on_no_warnings() {
        let source = LintRegressionSource;
        let raw = lines(&["  --> src/lib.rs:10:5", "= note: something"]);
        let input = source.extract(&raw, None);
        assert!(input.raw_signals.is_empty());
    }

    // ── CompileRegressionSource ───────────────────────────────────────────

    #[test]
    fn continuous_intake_compile_regression_captures_error_codes() {
        let source = CompileRegressionSource;
        let raw = lines(&[
            "error[E0277]: the trait bound `Foo: Bar` is not satisfied",
            "aborting due to 1 previous error",
            "  --> src/main.rs:5:10",
        ]);
        let input = source.extract(&raw, Some("build-99"));
        assert!(input.raw_signals.len() >= 2);
        assert_eq!(
            input.candidate_source,
            AutonomousCandidateSource::CompileRegression
        );
    }

    // ── RuntimePanicSource ────────────────────────────────────────────────

    #[test]
    fn continuous_intake_runtime_panic_captures_panic_lines() {
        let source = RuntimePanicSource;
        let raw = lines(&[
            "thread 'main' panicked at 'index out of bounds'",
            "note: run with RUST_BACKTRACE=1",
        ]);
        let input = source.extract(&raw, Some("incident-7"));
        assert_eq!(
            input.candidate_source,
            AutonomousCandidateSource::RuntimeIncident
        );
        // RuntimeIncident is not yet mapped to a BoundedTaskClass but signals are captured
        assert!(!input.raw_signals.is_empty());
    }

    // ── Unsupported / fail-closed behaviour ──────────────────────────────

    #[test]
    fn continuous_intake_runtime_panic_source_maps_to_runtime_incident() {
        let source = RuntimePanicSource::default();
        assert_eq!(
            source.candidate_source(),
            AutonomousCandidateSource::RuntimeIncident
        );
    }

    #[test]
    fn continuous_intake_all_sources_have_stable_names() {
        let sources: Vec<Box<dyn ContinuousIntakeSource>> = vec![
            Box::new(CiFailureSource),
            Box::new(TestRegressionSource),
            Box::new(LintRegressionSource),
            Box::new(CompileRegressionSource),
            Box::new(RuntimePanicSource),
        ];
        for src in &sources {
            assert!(!src.name().is_empty());
        }
    }
}
