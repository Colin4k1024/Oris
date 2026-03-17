//! Integration tests: full path IntakeEvent → Detect → Select stage entry.
//!
//! These tests verify that the pipeline integration wiring introduced in
//! `pipeline_integration.rs` produces the correct observable behaviour when
//! routed through `StandardEvolutionPipeline`.
//!
//! Run with:
//! ```bash
//! cargo test -p oris-evokernel --test evolution_feature_wiring
//! ```

use std::sync::Arc;

use oris_evokernel::adapters::RuntimeSignalExtractorAdapter;
use oris_evokernel::pipeline_integration::detect_from_intake_events;
use oris_evolution::{
    EvolutionPipeline, EvolutionPipelineConfig, GeneCandidate, PipelineContext, PipelineStageState,
    Selector, SelectorInput, StandardEvolutionPipeline,
};
use oris_intake::{IntakeEvent, IntakeSourceType, IssueSeverity};

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

fn make_event(
    source_type: IntakeSourceType,
    title: &str,
    description: &str,
    severity: IssueSeverity,
) -> IntakeEvent {
    IntakeEvent {
        event_id: uuid::Uuid::new_v4().to_string(),
        source_type,
        source_event_id: None,
        title: title.to_string(),
        description: description.to_string(),
        severity,
        signals: vec![],
        raw_payload: None,
        timestamp_ms: 0,
    }
}

/// Minimal no-op selector that always returns an empty candidate list.
struct NoopSelector;

impl Selector for NoopSelector {
    fn select(&self, _input: &SelectorInput) -> Vec<GeneCandidate> {
        vec![]
    }
}

/// Build a pipeline with Detect + Select enabled; all other stages disabled.
fn detect_select_pipeline() -> StandardEvolutionPipeline {
    let config = EvolutionPipelineConfig {
        enable_execute: false,
        enable_validate: false,
        enable_evaluate: false,
        enable_solidify: false,
        enable_reuse: false,
        ..EvolutionPipelineConfig::default()
    };
    StandardEvolutionPipeline::new(config, Arc::new(NoopSelector))
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

/// Full path: `IntakeEvent` with a `error[E0308]` compiler diagnostic →
/// `detect_from_intake_events` → `PipelineContext::signals` →
/// `StandardEvolutionPipeline::execute` → Detect stage `Completed`.
#[test]
fn intake_event_compiler_error_reaches_detect_stage() {
    let event = make_event(
        IntakeSourceType::LogFile,
        "cargo build failed",
        "error[E0308]: mismatched types\n  --> src/lib.rs:5:10\n   |\n5  |     let x: i32 = \"hello\";\n   |                   ^^^^^^^ expected `i32`, found `&str`",
        IssueSeverity::High,
    );

    let extractor = RuntimeSignalExtractorAdapter::new();
    let signals = detect_from_intake_events(&[event], &extractor);

    // At least one signal must be produced for a genuine compiler error.
    assert!(
        !signals.is_empty(),
        "detect_from_intake_events must produce signals for compiler error[E0308]"
    );

    let mut ctx = PipelineContext::default();
    ctx.signals = signals;

    let result = detect_select_pipeline()
        .execute(ctx)
        .expect("pipeline must not error");

    let detect_stage = result
        .stage_states
        .iter()
        .find(|s| s.stage_name == "detect")
        .expect("Detect stage must appear in stage_states");

    assert_eq!(
        detect_stage.state,
        PipelineStageState::Completed,
        "Detect stage must be Completed"
    );

    let select_stage = result
        .stage_states
        .iter()
        .find(|s| s.stage_name == "select")
        .expect("Select stage must appear in stage_states");

    assert!(
        matches!(
            select_stage.state,
            PipelineStageState::Completed | PipelineStageState::Skipped(_)
        ),
        "Select stage must be Completed or Skipped (no candidates); got {:?}",
        select_stage.state
    );
}

/// Full path: `IntakeEvent` with a runtime panic → Detect stage entry.
#[test]
fn intake_event_runtime_panic_reaches_detect_stage() {
    let event = make_event(
        IntakeSourceType::Sentry,
        "Worker thread panicked",
        "thread 'tokio-worker-2' panicked at 'index out of bounds: the len is 3 but the index is 5', src/worker.rs:42",
        IssueSeverity::Critical,
    );

    let extractor = RuntimeSignalExtractorAdapter::new();
    let signals = detect_from_intake_events(&[event], &extractor);

    assert!(
        !signals.is_empty(),
        "detect_from_intake_events must produce signals for runtime panic"
    );

    let mut ctx = PipelineContext::default();
    ctx.signals = signals;

    let result = detect_select_pipeline()
        .execute(ctx)
        .expect("pipeline must not error");

    let detect_stage = result
        .stage_states
        .iter()
        .find(|s| s.stage_name == "detect")
        .expect("Detect stage must appear");

    assert_eq!(detect_stage.state, PipelineStageState::Completed);
}

/// When `StandardEvolutionPipeline` is built with a `RuntimeSignalExtractorAdapter`
/// injected via `with_signal_extractor`, the adapter is called during the Detect
/// stage and its signals are merged into the pipeline context.
#[test]
fn pipeline_with_injected_extractor_calls_detect() {
    use oris_evokernel::pipeline_integration::intake_events_to_extractor_input;

    let event = make_event(
        IntakeSourceType::LogFile,
        "Build error",
        "error[E0425]: cannot find value `foo` in this scope\n  --> src/main.rs:10:5",
        IssueSeverity::High,
    );
    let extractor_input = intake_events_to_extractor_input(&[event]);

    let extractor = Arc::new(RuntimeSignalExtractorAdapter::new());
    let config = EvolutionPipelineConfig {
        enable_execute: false,
        enable_validate: false,
        enable_evaluate: false,
        enable_solidify: false,
        enable_reuse: false,
        ..EvolutionPipelineConfig::default()
    };
    let pipeline = StandardEvolutionPipeline::new(config, Arc::new(NoopSelector))
        .with_signal_extractor(extractor);

    // Supply the extractor input via PipelineContext::extractor_input so the
    // pipeline's Detect stage will call the injected extractor.
    let mut ctx = PipelineContext::default();
    ctx.extractor_input = Some(extractor_input);

    let result = pipeline.execute(ctx).expect("pipeline must not error");

    let detect = result
        .stage_states
        .iter()
        .find(|s| s.stage_name == "detect")
        .expect("Detect stage must appear");

    assert_eq!(
        detect.state,
        PipelineStageState::Completed,
        "Detect stage with injected extractor must complete"
    );
}
