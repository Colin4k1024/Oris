//! Runtime effect capture: canonical side-effect types and sink for the kernel.
//!
//! All side effects produced during execution are represented as [RuntimeEffect] and
//! logged to the active context via [EffectSink]. This ensures zero uncaptured
//! side effects leak into the execution state (replay and verification can rely on
//! a complete effect stream).

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::kernel::identity::RunId;

/// A single runtime side effect that the kernel (or adapters) must capture.
///
/// Every LLM call, tool call, state write, and interrupt raise is recorded as one
/// of these variants so that execution is fully auditable and replay-safe.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum RuntimeEffect {
    /// An LLM was invoked (provider + input).
    LLMCall { provider: String, input: Value },
    /// A tool was invoked (tool name + input).
    ToolCall { tool: String, input: Value },
    /// State was written (e.g. after a step; step_id and payload).
    StateWrite {
        step_id: Option<String>,
        payload: Value,
    },
    /// An interrupt was raised (e.g. human-in-the-loop; value for resolver).
    InterruptRaise { value: Value },
}

/// Sink for recording runtime effects for the active thread/run.
///
/// The kernel driver (and any code that produces side effects) should log every
/// [RuntimeEffect] through this trait so that nothing is uncaptured. Implementations
/// may append to a thread-local buffer, a run-scoped log, or a no-op for tests.
pub trait EffectSink: Send + Sync {
    /// Records one runtime effect for the given run.
    fn record(&self, run_id: &RunId, effect: &RuntimeEffect);
}

/// Effect sink that discards all effects (e.g. when capture is not needed).
#[derive(Debug, Default)]
pub struct NoopEffectSink;

impl EffectSink for NoopEffectSink {
    fn record(&self, _run_id: &RunId, _effect: &RuntimeEffect) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn runtime_effect_llm_call_roundtrip() {
        let e = RuntimeEffect::LLMCall {
            provider: "openai".to_string(),
            input: serde_json::json!({"model": "gpt-4"}),
        };
        let j = serde_json::to_value(&e).unwrap();
        let _: RuntimeEffect = serde_json::from_value(j).unwrap();
    }

    #[test]
    fn noop_effect_sink_accepts_all() {
        let sink = NoopEffectSink;
        let run_id: RunId = "test-run".into();
        sink.record(
            &run_id,
            &RuntimeEffect::StateWrite {
                step_id: None,
                payload: serde_json::json!(null),
            },
        );
    }

    #[test]
    fn effect_sink_can_count() {
        struct CountSink(AtomicUsize);
        impl EffectSink for CountSink {
            fn record(&self, _: &RunId, _: &RuntimeEffect) {
                self.0.fetch_add(1, Ordering::Relaxed);
            }
        }
        let sink = CountSink(AtomicUsize::new(0));
        let run_id: RunId = "test-run".into();
        sink.record(
            &run_id,
            &RuntimeEffect::ToolCall {
                tool: "t".into(),
                input: serde_json::json!(()),
            },
        );
        sink.record(
            &run_id,
            &RuntimeEffect::InterruptRaise {
                value: serde_json::json!(true),
            },
        );
        assert_eq!(sink.0.load(Ordering::Relaxed), 2);
    }
}
