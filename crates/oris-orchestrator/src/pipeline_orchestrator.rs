//! Pipeline orchestrator — end-to-end intake → execution automation (Issue #246).
//!
//! `PipelineOrchestrator` ties together three responsibilities:
//!
//! 1. Accept a batch of raw signal strings (from `oris-intake` or any other
//!    source) and classify them into a task class using a pluggable
//!    `ClassifierPort` trait.
//! 2. Delegate the actual execution (gene selection → mutation → sandbox) to a
//!    pluggable `EvolutionRunPort` trait, keeping the orchestrator independent
//!    of the concrete `StandardEvolutionPipeline` implementation.
//! 3. Pass the execution result through the fail-closed `AcceptanceGate` and
//!    either return an `AcceptableOutcome` or abort with a structured
//!    `AbortRecord`.
//!
//! All four fail-closed paths (PolicyDenied, ValidationFailed, UnsafePatch,
//! Timeout) produce an `Err(PipelineOrchestratorError)` that the caller is
//! required to handle — nothing is swallowed silently.

use crate::acceptance_gate::{
    AbortReason, AbortRecord, AcceptableOutcome, AcceptanceGate, AcceptanceGateError,
    PipelineOutcomeView,
};

// ---------------------------------------------------------------------------
// Ports (trait abstractions)
// ---------------------------------------------------------------------------

/// Port for classifying a slice of raw signal strings into a task-class id.
///
/// The concrete implementation can call `oris_evolution::TaskClassMatcher`
/// directly; tests supply a stub.
pub trait ClassifierPort: Send + Sync {
    /// Return the `task_class_id` best matching the supplied signals, or
    /// `None` when no class reaches the minimum overlap threshold.
    fn classify(&self, signals: &[String]) -> Option<String>;
}

/// Port representing a single bounded execution of the evolution pipeline
/// (Detect → Select → Mutate → Execute → Validate → Evaluate).
///
/// The concrete implementation drives `StandardEvolutionPipeline`; tests
/// supply an `InMemoryEvolutionRun` that returns a pre-configured result.
pub trait EvolutionRunPort: Send + Sync {
    /// Run the pipeline for the given signals and task class.
    ///
    /// Returns a `PipelineOutcomeView` with the boolean flags that the
    /// `AcceptanceGate` needs, or `None` when the run times out externally
    /// (before the gate is even reached).
    fn run(&self, signals: &[String], task_class_id: Option<&str>) -> Option<PipelineOutcomeView>;
}

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Configuration for `PipelineOrchestrator`.
#[derive(Clone, Debug)]
pub struct OrchestratorConfig {
    /// When no classifier is provided or no class is matched, the pipeline
    /// still runs with `task_class_id = None`.
    pub require_task_class: bool,
}

impl Default for OrchestratorConfig {
    fn default() -> Self {
        Self {
            require_task_class: false,
        }
    }
}

// ---------------------------------------------------------------------------
// Error types
// ---------------------------------------------------------------------------

/// Errors returned by `PipelineOrchestrator::run`.
#[derive(Clone, Debug)]
pub enum PipelineOrchestratorError {
    /// No signals were provided.
    EmptySignals,
    /// The run timed out before producing any result.
    RunTimeout { run_id: String },
    /// The acceptance gate rejected the run.
    GateRejected {
        abort: AbortReason,
        abort_record: AbortRecord,
    },
}

impl std::fmt::Display for PipelineOrchestratorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptySignals => write!(f, "PipelineOrchestrator: signal list is empty"),
            Self::RunTimeout { run_id } => {
                write!(f, "PipelineOrchestrator: run '{}' timed out", run_id)
            }
            Self::GateRejected { abort, .. } => {
                write!(
                    f,
                    "PipelineOrchestrator: acceptance gate aborted — {}",
                    abort
                )
            }
        }
    }
}

impl std::error::Error for PipelineOrchestratorError {}

// ---------------------------------------------------------------------------
// Orchestrator
// ---------------------------------------------------------------------------

/// End-to-end pipeline orchestrator.
///
/// Call `run(signals)` to drive the full intake → execution → gate flow.  The
/// call is synchronous (blocking); async wrappers can be layered by callers.
pub struct PipelineOrchestrator {
    classifier: Option<Box<dyn ClassifierPort>>,
    evolution_run: Box<dyn EvolutionRunPort>,
    config: OrchestratorConfig,
}

impl PipelineOrchestrator {
    /// Create an orchestrator with the mandatory evolution-run port and an
    /// optional classifier.
    pub fn new(
        evolution_run: Box<dyn EvolutionRunPort>,
        classifier: Option<Box<dyn ClassifierPort>>,
        config: OrchestratorConfig,
    ) -> Self {
        Self {
            classifier,
            evolution_run,
            config,
        }
    }

    /// Drive the full pipeline and return an `AcceptableOutcome` or abort.
    ///
    /// **Fail-closed**: every non-success path returns `Err(...)`.
    pub fn run(
        &self,
        run_id: impl Into<String>,
        signals: &[String],
    ) -> Result<AcceptableOutcome, PipelineOrchestratorError> {
        let run_id = run_id.into();

        if signals.is_empty() {
            return Err(PipelineOrchestratorError::EmptySignals);
        }

        // Step 1: classify signals (optional)
        let task_class_id = self.classifier.as_ref().and_then(|c| c.classify(signals));

        // Step 2: execute the evolution pipeline
        let outcome_view = self
            .evolution_run
            .run(signals, task_class_id.as_deref())
            .ok_or_else(|| PipelineOrchestratorError::RunTimeout {
                run_id: run_id.clone(),
            })?;

        // Step 3: pass through the fail-closed acceptance gate
        AcceptanceGate::evaluate(&outcome_view).map_err(|gate_err: AcceptanceGateError| {
            let record = AbortRecord::now(
                &run_id,
                gate_err.abort.clone(),
                &gate_err.detail,
                signals.to_vec(),
            );
            PipelineOrchestratorError::GateRejected {
                abort: gate_err.abort,
                abort_record: record,
            }
        })
    }
}

// ---------------------------------------------------------------------------
// In-memory stubs for tests
// ---------------------------------------------------------------------------

/// An `EvolutionRunPort` stub whose outcome is fully pre-configured.
///
/// All flag fields default to `true` (green).  Individual fields can be set
/// to `false` to simulate specific fail-closed scenarios.
#[derive(Clone, Debug)]
pub struct InMemoryEvolutionRun {
    pub sandbox_safe: bool,
    pub validation_passed: bool,
    pub policy_passed: bool,
    pub within_time_budget: bool,
    /// When `simulate_timeout` is `true`, `run()` returns `None` to model an
    /// external timeout (before the gate is reached).
    pub simulate_timeout: bool,
}

impl Default for InMemoryEvolutionRun {
    fn default() -> Self {
        Self {
            sandbox_safe: true,
            validation_passed: true,
            policy_passed: true,
            within_time_budget: true,
            simulate_timeout: false,
        }
    }
}

impl EvolutionRunPort for InMemoryEvolutionRun {
    fn run(&self, signals: &[String], _task_class_id: Option<&str>) -> Option<PipelineOutcomeView> {
        if self.simulate_timeout {
            return None;
        }
        Some(PipelineOutcomeView {
            run_id: "stub-run".to_string(),
            signals: signals.to_vec(),
            sandbox_safe: self.sandbox_safe,
            validation_passed: self.validation_passed,
            policy_passed: self.policy_passed,
            within_time_budget: self.within_time_budget,
        })
    }
}

/// A `ClassifierPort` stub that always returns a fixed task-class id.
pub struct FixedClassifier(pub Option<String>);

impl ClassifierPort for FixedClassifier {
    fn classify(&self, _signals: &[String]) -> Option<String> {
        self.0.clone()
    }
}
