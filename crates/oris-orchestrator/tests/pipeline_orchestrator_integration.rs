//! Integration tests for Issue #246: end-to-end pipeline orchestration.
//!
//! Covers the three acceptance-criteria items:
//!
//! AC-1: intake → execution chain runs without manual intervention (success path)
//! AC-2: fail-closed — each of the four abort reasons correctly aborts and records
//!       (PolicyDenied, ValidationFailed, UnsafePatch, Timeout)
//! AC-3: integration test covering success path + every fail-closed path

use oris_orchestrator::acceptance_gate::AbortReason;
use oris_orchestrator::pipeline_orchestrator::{
    FixedClassifier, InMemoryEvolutionRun, OrchestratorConfig, PipelineOrchestrator,
    PipelineOrchestratorError,
};

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

fn signals() -> Vec<String> {
    vec![
        "error[E0425]: cannot find value `foo` in this scope".to_string(),
        "test integration ... FAILED".to_string(),
    ]
}

fn orchestrator_with(run: InMemoryEvolutionRun) -> PipelineOrchestrator {
    PipelineOrchestrator::new(
        Box::new(run),
        Some(Box::new(FixedClassifier(Some(
            "missing-import".to_string(),
        )))),
        OrchestratorConfig::default(),
    )
}

// ---------------------------------------------------------------------------
// AC-1 / AC-3: success path
// ---------------------------------------------------------------------------

/// success path: all flags green → AcceptableOutcome returned, no abort
#[test]
fn test_success_path_returns_acceptable_outcome() {
    let run = InMemoryEvolutionRun::default(); // all green
    let orchestrator = orchestrator_with(run);
    let result = orchestrator.run("run-success", &signals());
    assert!(
        result.is_ok(),
        "expect AcceptableOutcome, got: {:?}",
        result.err()
    );
    let outcome = result.unwrap();
    assert_eq!(outcome.run_id, "stub-run");
    assert!(!outcome.signals.is_empty());
}

/// success path with classifier: task_class_id is threaded to the run port
#[test]
fn test_success_path_with_classifier() {
    let run = InMemoryEvolutionRun::default();
    let orchestrator = PipelineOrchestrator::new(
        Box::new(run),
        Some(Box::new(FixedClassifier(Some("test-failure".to_string())))),
        OrchestratorConfig::default(),
    );
    let result = orchestrator.run("run-classified", &signals());
    assert!(result.is_ok());
}

/// success path without classifier: pipeline still runs
#[test]
fn test_success_path_no_classifier() {
    let run = InMemoryEvolutionRun::default();
    let orchestrator =
        PipelineOrchestrator::new(Box::new(run), None, OrchestratorConfig::default());
    let result = orchestrator.run("run-no-class", &signals());
    assert!(result.is_ok());
}

/// empty signals are rejected before the pipeline is invoked
#[test]
fn test_empty_signals_returns_error() {
    let orchestrator = orchestrator_with(InMemoryEvolutionRun::default());
    let result = orchestrator.run("run-empty", &[]);
    assert!(matches!(
        result,
        Err(PipelineOrchestratorError::EmptySignals)
    ));
}

// ---------------------------------------------------------------------------
// AC-2 / AC-3: fail-closed paths
// ---------------------------------------------------------------------------

/// fail-closed: PolicyDenied — governance policy rejected the run
#[test]
fn test_fail_closed_policy_denied() {
    let run = InMemoryEvolutionRun {
        policy_passed: false,
        ..Default::default()
    };
    let orchestrator = orchestrator_with(run);
    let err = orchestrator.run("run-policy", &signals()).unwrap_err();
    match &err {
        PipelineOrchestratorError::GateRejected {
            abort,
            abort_record,
        } => {
            assert_eq!(*abort, AbortReason::PolicyDenied);
            assert_eq!(abort_record.reason, AbortReason::PolicyDenied);
            assert!(
                !abort_record.signals.is_empty(),
                "abort record must carry signals"
            );
            assert!(
                abort_record.aborted_at_ms > 0,
                "abort timestamp must be set"
            );
        }
        other => panic!("expected GateRejected(PolicyDenied), got {:?}", other),
    }
}

/// fail-closed: ValidationFailed — post-execution build/test verdict negative
#[test]
fn test_fail_closed_validation_failed() {
    let run = InMemoryEvolutionRun {
        validation_passed: false,
        ..Default::default()
    };
    let orchestrator = orchestrator_with(run);
    let err = orchestrator.run("run-valif", &signals()).unwrap_err();
    match &err {
        PipelineOrchestratorError::GateRejected { abort, .. } => {
            assert_eq!(*abort, AbortReason::ValidationFailed);
        }
        other => panic!("expected GateRejected(ValidationFailed), got {:?}", other),
    }
    // Error message must mention the reason
    assert!(err.to_string().contains("ValidationFailed"));
}

/// fail-closed: UnsafePatch — sandbox reported out-of-scope file modification
#[test]
fn test_fail_closed_unsafe_patch() {
    let run = InMemoryEvolutionRun {
        sandbox_safe: false,
        ..Default::default()
    };
    let orchestrator = orchestrator_with(run);
    let err = orchestrator.run("run-unsafe", &signals()).unwrap_err();
    match &err {
        PipelineOrchestratorError::GateRejected { abort, .. } => {
            assert_eq!(*abort, AbortReason::UnsafePatch);
        }
        other => panic!("expected GateRejected(UnsafePatch), got {:?}", other),
    }
}

/// fail-closed: Timeout — run exceeded its budget (gate-internal Timeout flag)
#[test]
fn test_fail_closed_timeout_via_gate() {
    let run = InMemoryEvolutionRun {
        within_time_budget: false,
        ..Default::default()
    };
    let orchestrator = orchestrator_with(run);
    let err = orchestrator
        .run("run-timeout-gate", &signals())
        .unwrap_err();
    match &err {
        PipelineOrchestratorError::GateRejected { abort, .. } => {
            assert_eq!(*abort, AbortReason::Timeout);
        }
        other => panic!("expected GateRejected(Timeout), got {:?}", other),
    }
}

/// fail-closed: Timeout — run never returns (external timeout, None returned)
#[test]
fn test_fail_closed_timeout_via_run_none() {
    let run = InMemoryEvolutionRun {
        simulate_timeout: true,
        ..Default::default()
    };
    let orchestrator = orchestrator_with(run);
    let err = orchestrator.run("run-ext-timeout", &signals()).unwrap_err();
    assert!(
        matches!(err, PipelineOrchestratorError::RunTimeout { .. }),
        "expected RunTimeout, got: {}",
        err
    );
}

/// fail-closed ordering: Timeout wins over PolicyDenied + UnsafePatch + ValidationFailed
#[test]
fn test_fail_closed_timeout_wins_all_other_failures() {
    let run = InMemoryEvolutionRun {
        sandbox_safe: false,
        validation_passed: false,
        policy_passed: false,
        within_time_budget: false,
        simulate_timeout: false, // gate sees all flags
    };
    let orchestrator = orchestrator_with(run);
    let err = orchestrator.run("run-all-bad", &signals()).unwrap_err();
    match &err {
        PipelineOrchestratorError::GateRejected { abort, .. } => {
            assert_eq!(
                *abort,
                AbortReason::Timeout,
                "Timeout must take precedence over all other gate failures"
            );
        }
        other => panic!("expected GateRejected(Timeout), got {:?}", other),
    }
}
