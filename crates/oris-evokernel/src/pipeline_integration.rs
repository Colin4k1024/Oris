//! Pipeline integration: wires `IntakeEvent` → `RuntimeSignal` →
//! `SignalExtractorPort::extract()` → `StandardEvolutionPipeline` Detect stage.
//!
//! # Overview
//!
//! `StandardEvolutionPipeline` accepts signals through its Detect stage via a
//! `SignalExtractorPort`.  The signal extractor (already implemented as
//! `RuntimeSignalExtractorAdapter` in `adapters.rs`) translates raw
//! `SignalExtractorInput` fields (compiler output / stack trace / logs) into
//! `EvolutionSignal`s.
//!
//! This module bridges the gap on the *intake* side: it turns
//! `oris_intake::IntakeEvent`s into a `SignalExtractorInput` so that
//! the pipeline Detect stage can be driven directly from webhook events.
//!
//! ```no_run
//! use std::sync::Arc;
//! use oris_evolution::{EvolutionPipelineConfig, StandardEvolutionPipeline};
//! use oris_evokernel::adapters::RuntimeSignalExtractorAdapter;
//! use oris_evokernel::pipeline_integration::detect_from_intake_events;
//! use oris_intake::IntakeEvent;
//! # use oris_evolution::Selector;
//! # fn dummy_selector() -> Arc<dyn Selector> { unimplemented!() }
//!
//! let events: Vec<IntakeEvent> = vec![/* ... */];
//! let extractor = Arc::new(RuntimeSignalExtractorAdapter::new());
//! let signals = detect_from_intake_events(&events, extractor.as_ref());
//! ```

use oris_evolution::{EvolutionSignal, SignalExtractorInput, SignalExtractorPort};
use oris_intake::IntakeEvent;

/// Convert a slice of `IntakeEvent`s into a `SignalExtractorInput`.
///
/// The mapping strategy:
/// - Events whose title or description contain rustc compiler error/warning markers
///   (e.g. `error[EXXXX]:`) are placed in `compiler_output`.
/// - Events that contain typical panic / stack-trace markers
///   are placed in `stack_trace`.
/// - All remaining events are serialised as log lines and placed in `logs`.
///
/// Signals from all three buckets are then deduplicated by the underlying
/// `RuntimeSignalExtractor`.
pub fn intake_events_to_extractor_input(events: &[IntakeEvent]) -> SignalExtractorInput {
    let mut compiler_parts: Vec<String> = Vec::new();
    let mut stack_parts: Vec<String> = Vec::new();
    let mut log_parts: Vec<String> = Vec::new();

    for event in events {
        let content = format!("{}\n{}", event.title, event.description);

        let is_compiler = content.contains("error[E")
            || content.contains("error[W")
            || content.contains("warning[W")
            || content.contains("cannot find")
            || content.contains("mismatched types");
        let is_panic = !is_compiler
            && content.contains("thread '")
            && (content.contains("panicked at") || content.contains("stack backtrace"));

        if is_compiler {
            compiler_parts.push(content);
        } else if is_panic {
            stack_parts.push(content);
        } else {
            log_parts.push(format!(
                "[{}] [{}] {}",
                event.timestamp_ms, event.severity, content
            ));
        }
    }

    SignalExtractorInput {
        compiler_output: if compiler_parts.is_empty() {
            None
        } else {
            Some(compiler_parts.join("\n"))
        },
        stack_trace: if stack_parts.is_empty() {
            None
        } else {
            Some(stack_parts.join("\n"))
        },
        logs: if log_parts.is_empty() {
            None
        } else {
            Some(log_parts.join("\n"))
        },
        extra: serde_json::json!({}),
    }
}

/// Run the Detect stage for a batch of `IntakeEvent`s using the provided
/// `SignalExtractorPort`.
///
/// Returns the list of `EvolutionSignal`s ready to be injected into
/// `PipelineContext::signals` before calling `StandardEvolutionPipeline::execute`.
pub fn detect_from_intake_events(
    events: &[IntakeEvent],
    extractor: &dyn SignalExtractorPort,
) -> Vec<EvolutionSignal> {
    let input = intake_events_to_extractor_input(events);
    let mut signals = extractor.extract(&input);
    // Only keep signals above a minimal confidence floor so that noise from
    // low-confidence log lines does not pollute the Select stage.
    signals.retain(|s| s.confidence >= 0.3);
    signals
}

// ─────────────────────────────────────────────────────────────────────────────
// Unit tests
// ─────────────────────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::RuntimeSignalExtractorAdapter;
    use oris_intake::{IntakeEvent, IntakeSourceType, IssueSeverity};

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

    /// Simulating a `error[E0308]` compiler diagnostic arriving via intake
    /// must produce at least one `CompilerDiagnostic`-class `EvolutionSignal`
    /// with `SignalType::ErrorPattern`.
    #[test]
    fn compiler_diagnostic_produces_signal_ready() {
        // Content contains "error[E..." so it will be routed to compiler_output.
        let event = make_event(
            IntakeSourceType::LogFile,
            "Build failed",
            "error[E0308]: mismatched types\n  --> src/lib.rs:10:5\n   |\n10 |     let x: i32 = \"hello\";\n   |            ---   ^^^^^^^ expected `i32`, found `&str`",
            IssueSeverity::High,
        );

        let extractor = RuntimeSignalExtractorAdapter::new();
        let signals = detect_from_intake_events(&[event], &extractor);

        assert!(
            !signals.is_empty(),
            "expected at least one EvolutionSignal for compiler error; got none"
        );
        // Verify the signal represents an ErrorPattern (compiler path).
        let has_error_pattern = signals.iter().any(|s| {
            matches!(
                &s.signal_type,
                oris_evolution::evolver::SignalType::ErrorPattern { .. }
            )
        });
        assert!(
            has_error_pattern,
            "expected an ErrorPattern signal for compiler diagnostic, got: {:?}",
            signals
                .iter()
                .map(|s| format!("{:?}", s.signal_type))
                .collect::<Vec<_>>()
        );
    }

    /// A runtime panic arriving as an intake event must route through the
    /// stack-trace path and produce at least one `EvolutionSignal`.
    #[test]
    fn runtime_panic_produces_evolution_signal() {
        // Content contains "thread '...panicked at" so it routes to stack_trace.
        let event = make_event(
            IntakeSourceType::Sentry,
            "Worker panicked",
            "thread 'main' panicked at 'index out of bounds: the len is 3 but the index is 5', src/worker.rs:42",
            IssueSeverity::Critical,
        );

        let extractor = RuntimeSignalExtractorAdapter::new();
        let signals = detect_from_intake_events(&[event], &extractor);

        assert!(
            !signals.is_empty(),
            "expected at least one EvolutionSignal for runtime panic; got none"
        );
    }

    /// Intake events that carry no recognisable diagnostic markers must not
    /// produce false-positive Evolution signals with high confidence.
    #[test]
    fn noise_event_does_not_produce_high_confidence_signal() {
        let event = make_event(
            IntakeSourceType::Github,
            "Dependabot security update",
            "Bump serde from 1.0.195 to 1.0.197",
            IssueSeverity::Low,
        );

        let extractor = RuntimeSignalExtractorAdapter::new();
        let signals = detect_from_intake_events(&[event], &extractor);

        // Low-content noise events may produce zero signals or only very low
        // confidence ones.  Assert none exceed 0.8 confidence.
        let high_conf: Vec<_> = signals.iter().filter(|s| s.confidence > 0.8).collect();
        assert!(
            high_conf.is_empty(),
            "noise event should not produce high-confidence signals; got: {:?}",
            high_conf
        );
    }

    /// `intake_events_to_extractor_input` routing: compiler events land in
    /// `compiler_output`, panic events in `stack_trace`, generic events in `logs`.
    #[test]
    fn intake_events_routing() {
        // compiler_output: contains "error[E"
        let compiler_event = make_event(
            IntakeSourceType::LogFile,
            "Build error",
            "error[E0425]: cannot find value `x`",
            IssueSeverity::High,
        );
        // stack_trace: contains thread panic pattern
        let panic_event = make_event(
            IntakeSourceType::Sentry,
            "Panic detected",
            "thread 'tokio-worker' panicked at 'assertion failed', src/main.rs:5\nstack backtrace:\n   0: std::panicking::begin_panic",
            IssueSeverity::Critical,
        );
        // logs: generic content
        let log_event = make_event(
            IntakeSourceType::Github,
            "PR merged",
            "feat: add new feature",
            IssueSeverity::Info,
        );

        let input = intake_events_to_extractor_input(&[compiler_event, panic_event, log_event]);

        assert!(
            input.compiler_output.is_some(),
            "compiler event must populate compiler_output"
        );
        assert!(
            input.stack_trace.is_some(),
            "panic event must populate stack_trace"
        );
        assert!(input.logs.is_some(), "log event must populate logs");
    }

    /// Full pipeline path: IntakeEvent → Detect → Select stage entry point.
    ///
    /// This test wires an `IntakeEvent` with a compiler diagnostic through the
    /// `detect_from_intake_events` helper, then feeds the resulting signals
    /// directly into `StandardEvolutionPipeline` via `PipelineContext::signals`.
    /// It asserts that the Detect stage completes and the Select stage starts.
    #[test]
    fn intake_event_detect_select_pipeline_path() {
        use oris_evolution::{
            EvolutionPipeline, EvolutionPipelineConfig, PipelineContext, PipelineStageState,
            StandardEvolutionPipeline,
        };
        use oris_evolution::{GeneCandidate, Selector, SelectorInput};
        use std::sync::Arc;

        // Minimal no-op selector that always returns no candidates.
        struct NoopSelector;
        impl Selector for NoopSelector {
            fn select(&self, _input: &SelectorInput) -> Vec<GeneCandidate> {
                vec![]
            }
        }

        // content has "error[E" → routed to compiler_output
        let compiler_event = make_event(
            IntakeSourceType::LogFile,
            "cargo build failed",
            "error[E0308]: mismatched types\n  --> src/lib.rs:5:10",
            IssueSeverity::High,
        );

        let extractor = RuntimeSignalExtractorAdapter::new();
        let signals = detect_from_intake_events(&[compiler_event], &extractor);

        // Build a pipeline that only runs Detect + Select (other stages disabled).
        let config = EvolutionPipelineConfig {
            enable_execute: false,
            enable_validate: false,
            enable_evaluate: false,
            enable_solidify: false,
            enable_reuse: false,
            ..EvolutionPipelineConfig::default()
        };
        let pipeline = StandardEvolutionPipeline::new(config, Arc::new(NoopSelector));

        // Pre-populate context with the signals produced by detect_from_intake_events.
        let mut ctx = PipelineContext::default();
        ctx.signals = signals.clone();

        let result = pipeline.execute(ctx).expect("pipeline must not error");

        // Detect and Select stages must complete (or skip with no candidates).
        let detect_state = result
            .stage_states
            .iter()
            .find(|s| s.stage_name == "detect");
        assert!(
            detect_state.is_some(),
            "Detect stage must appear in stage_states"
        );
        assert_eq!(
            detect_state.unwrap().state,
            PipelineStageState::Completed,
            "Detect stage must complete"
        );

        let select_state = result
            .stage_states
            .iter()
            .find(|s| s.stage_name == "select");
        assert!(
            select_state.is_some(),
            "Select stage must appear in stage_states"
        );
    }
}
