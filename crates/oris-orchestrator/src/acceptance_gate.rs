//! Fail-closed acceptance gate for the evolution pipeline (Issue #246).
//!
//! Any execution path that cannot be positively verified as safe and correct
//! must abort immediately and record the reason.  The gate never silently
//! passes an unverified mutation.
//!
//! ## Fail-closed principle
//!
//! If `evaluate()` returns `Ok(AcceptableOutcome)` the caller may proceed.
//! For _every_ error variant the caller must abort the pipeline run and
//! persist an `AbortRecord` before continuing.

use serde::{Deserialize, Serialize};

/// The reason a mutation run was aborted by the acceptance gate.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AbortReason {
    /// A governance or safety policy explicitly denied the mutation.
    PolicyDenied,
    /// The post-execution validation step returned a failing verdict.
    ValidationFailed,
    /// The sandbox detected an unsafe or out-of-scope file modification.
    UnsafePatch,
    /// The pipeline or a single stage exceeded its allotted time budget.
    Timeout,
}

impl std::fmt::Display for AbortReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AbortReason::PolicyDenied => write!(f, "PolicyDenied"),
            AbortReason::ValidationFailed => write!(f, "ValidationFailed"),
            AbortReason::UnsafePatch => write!(f, "UnsafePatch"),
            AbortReason::Timeout => write!(f, "Timeout"),
        }
    }
}

/// Structured record persisted when a pipeline run is aborted.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AbortRecord {
    /// Unique run identifier (e.g. session-id or UUID).
    pub run_id: String,
    /// The specific reason the run was aborted.
    pub reason: AbortReason,
    /// Human-readable detail message.
    pub detail: String,
    /// Signal strings that triggered the run (for audit trail).
    pub signals: Vec<String>,
    /// Unix-millisecond timestamp of the abort.
    pub aborted_at_ms: i64,
}

impl AbortRecord {
    /// Create a new abort record with the current wall-clock timestamp.
    pub fn now(
        run_id: impl Into<String>,
        reason: AbortReason,
        detail: impl Into<String>,
        signals: Vec<String>,
    ) -> Self {
        use std::time::{SystemTime, UNIX_EPOCH};
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        Self {
            run_id: run_id.into(),
            reason,
            detail: detail.into(),
            signals,
            aborted_at_ms: ts,
        }
    }
}

/// The outcome returned by the gate when a run is _acceptable_.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AcceptableOutcome {
    /// Run identifier.
    pub run_id: String,
    /// Signals that were evaluated.
    pub signals: Vec<String>,
}

/// Error returned when the gate rejects a run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AcceptanceGateError {
    pub abort: AbortReason,
    pub detail: String,
}

impl AcceptanceGateError {
    pub fn new(abort: AbortReason, detail: impl Into<String>) -> Self {
        Self {
            abort,
            detail: detail.into(),
        }
    }
}

impl std::fmt::Display for AcceptanceGateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "AcceptanceGate aborted: {} — {}",
            self.abort, self.detail
        )
    }
}

impl std::error::Error for AcceptanceGateError {}

/// A snapshot of a pipeline execution outcome as seen by the gate.
///
/// The orchestrator converts its pipeline result into this view, which the
/// gate evaluates without knowing about the concrete pipeline types.
#[derive(Clone, Debug)]
pub struct PipelineOutcomeView {
    pub run_id: String,
    pub signals: Vec<String>,
    /// True if the execution sandbox confirmed the patch is safe.
    pub sandbox_safe: bool,
    /// True if the validation step passed.
    pub validation_passed: bool,
    /// True if governance / policy check passed.
    pub policy_passed: bool,
    /// True if the run completed within the allowed time budget.
    pub within_time_budget: bool,
}

impl PipelineOutcomeView {
    /// Convenience constructor for a fully-green outcome.
    pub fn all_green(run_id: impl Into<String>, signals: Vec<String>) -> Self {
        Self {
            run_id: run_id.into(),
            signals,
            sandbox_safe: true,
            validation_passed: true,
            policy_passed: true,
            within_time_budget: true,
        }
    }
}

/// The fail-closed acceptance gate.
///
/// Evaluation order:  Timeout → PolicyDenied → UnsafePatch → ValidationFailed
/// The first failing condition wins; all remaining checks are skipped.
pub struct AcceptanceGate;

impl AcceptanceGate {
    /// Evaluate a pipeline outcome and return `Ok(AcceptableOutcome)` or an
    /// `AcceptanceGateError` describing the first failing condition.
    ///
    /// **fail-closed**: any false flag aborts and records the reason.
    pub fn evaluate(view: &PipelineOutcomeView) -> Result<AcceptableOutcome, AcceptanceGateError> {
        // 1. Check time budget first — a timed-out run cannot be trusted.
        if !view.within_time_budget {
            return Err(AcceptanceGateError::new(
                AbortReason::Timeout,
                format!("run '{}' exceeded its time budget", view.run_id),
            ));
        }

        // 2. Policy gate — governance must approve before any sandbox check.
        if !view.policy_passed {
            return Err(AcceptanceGateError::new(
                AbortReason::PolicyDenied,
                format!("governance policy denied run '{}'", view.run_id),
            ));
        }

        // 3. Sandbox safety — unsafe filesystem changes must be rejected.
        if !view.sandbox_safe {
            return Err(AcceptanceGateError::new(
                AbortReason::UnsafePatch,
                format!("sandbox detected unsafe patch in run '{}'", view.run_id),
            ));
        }

        // 4. Validation — test/build verdict must pass.
        if !view.validation_passed {
            return Err(AcceptanceGateError::new(
                AbortReason::ValidationFailed,
                format!("validation failed for run '{}'", view.run_id),
            ));
        }

        Ok(AcceptableOutcome {
            run_id: view.run_id.clone(),
            signals: view.signals.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn green(run_id: &str) -> PipelineOutcomeView {
        PipelineOutcomeView::all_green(run_id, vec!["sig".to_string()])
    }

    #[test]
    fn test_all_green_is_accepted() {
        let result = AcceptanceGate::evaluate(&green("run-1"));
        assert!(result.is_ok());
        assert_eq!(result.unwrap().run_id, "run-1");
    }

    #[test]
    fn test_policy_denied_aborts() {
        let mut view = green("run-2");
        view.policy_passed = false;
        let err = AcceptanceGate::evaluate(&view).unwrap_err();
        assert_eq!(err.abort, AbortReason::PolicyDenied);
    }

    #[test]
    fn test_validation_failed_aborts() {
        let mut view = green("run-3");
        view.validation_passed = false;
        let err = AcceptanceGate::evaluate(&view).unwrap_err();
        assert_eq!(err.abort, AbortReason::ValidationFailed);
    }

    #[test]
    fn test_unsafe_patch_aborts() {
        let mut view = green("run-4");
        view.sandbox_safe = false;
        let err = AcceptanceGate::evaluate(&view).unwrap_err();
        assert_eq!(err.abort, AbortReason::UnsafePatch);
    }

    #[test]
    fn test_timeout_aborts() {
        let mut view = green("run-5");
        view.within_time_budget = false;
        let err = AcceptanceGate::evaluate(&view).unwrap_err();
        assert_eq!(err.abort, AbortReason::Timeout);
    }

    #[test]
    fn test_timeout_wins_over_policy_denied() {
        // Evaluation order: Timeout is checked before PolicyDenied
        let view = PipelineOutcomeView {
            run_id: "run-6".to_string(),
            signals: vec![],
            sandbox_safe: false,
            validation_passed: false,
            policy_passed: false,
            within_time_budget: false,
        };
        let err = AcceptanceGate::evaluate(&view).unwrap_err();
        assert_eq!(
            err.abort,
            AbortReason::Timeout,
            "Timeout must win over all other failures"
        );
    }

    #[test]
    fn test_abort_record_creation() {
        let record = AbortRecord::now(
            "run-7",
            AbortReason::PolicyDenied,
            "policy foo denied",
            vec!["error[E0425]".to_string()],
        );
        assert_eq!(record.run_id, "run-7");
        assert_eq!(record.reason, AbortReason::PolicyDenied);
        assert!(!record.signals.is_empty());
        assert!(record.aborted_at_ms > 0);
    }
}
