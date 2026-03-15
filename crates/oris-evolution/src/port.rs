//! Port traits for Detect and Execute stage integration.
//!
//! These traits let `StandardEvolutionPipeline` accept injected implementations
//! without directly depending on `oris-evokernel` or `oris-sandbox` (which
//! already depend on `oris-evolution`, so backward imports would be circular).
//!
//! Canonical implementations live in `oris-evokernel`:
//! - `RuntimeSignalExtractorAdapter` implements `SignalExtractorPort`
//! - `LocalSandboxAdapter` implements `SandboxPort`

use serde_json::Value;

use crate::core::PreparedMutation;
use crate::evolver::EvolutionSignal;

/// Input passed to the signal extractor during the Detect stage.
#[derive(Clone, Debug, Default)]
pub struct SignalExtractorInput {
    /// Raw compiler output (rustc / cargo build stderr)
    pub compiler_output: Option<String>,
    /// Stack trace text
    pub stack_trace: Option<String>,
    /// Execution log lines
    pub logs: Option<String>,
    /// Arbitrary extra context (serialised JSON)
    pub extra: Value,
}

/// Outcome of a sandbox execution during the Execute stage.
#[derive(Clone, Debug)]
pub struct SandboxExecutionResult {
    /// Whether the sandbox apply succeeded
    pub success: bool,
    /// Standard output captured during execution
    pub stdout: String,
    /// Standard error captured during execution
    pub stderr: String,
    /// Wall-clock duration in milliseconds
    pub duration_ms: u64,
    /// Human-readable message
    pub message: String,
}

impl SandboxExecutionResult {
    /// Convenience constructor for a successful result.
    pub fn success(stdout: impl Into<String>, duration_ms: u64) -> Self {
        Self {
            success: true,
            stdout: stdout.into(),
            stderr: String::new(),
            duration_ms,
            message: "Mutation executed successfully".into(),
        }
    }

    /// Convenience constructor for a failed result.
    pub fn failure(
        stderr: impl Into<String>,
        message: impl Into<String>,
        duration_ms: u64,
    ) -> Self {
        Self {
            success: false,
            stdout: String::new(),
            stderr: stderr.into(),
            duration_ms,
            message: message.into(),
        }
    }

    /// Serialise to a `serde_json::Value` suitable for `PipelineContext.execution_result`.
    pub fn to_json(&self) -> Value {
        serde_json::json!({
            "success": self.success,
            "stdout": self.stdout,
            "stderr": self.stderr,
            "duration_ms": self.duration_ms,
            "message": self.message,
        })
    }
}

/// Trait for signal extractors that can be injected into the Detect stage.
///
/// Implement this in `oris-evokernel` (or any crate that does not form
/// a circular dependency with `oris-evolution`) and pass an
/// `Arc<dyn SignalExtractorPort>` when constructing `StandardEvolutionPipeline`.
pub trait SignalExtractorPort: Send + Sync {
    /// Extract evolution signals from the given runtime input.
    ///
    /// Returns a (possibly empty) list of signals. The implementation is
    /// responsible for deduplication and confidence scoring.
    fn extract(&self, input: &SignalExtractorInput) -> Vec<EvolutionSignal>;
}

/// Trait for sandboxes that can be injected into the Execute stage.
///
/// Implement this in `oris-evokernel` and pass an `Arc<dyn SandboxPort>`
/// when constructing `StandardEvolutionPipeline`. The trait uses a
/// synchronous contract so that `EvolutionPipeline::execute` remains
/// synchronous at the pipeline level; async adapters should block internally.
pub trait SandboxPort: Send + Sync {
    /// Apply the first mutation proposal (represented as a `PreparedMutation`)
    /// inside a sandbox and return the execution result.
    fn execute(&self, mutation: &PreparedMutation) -> SandboxExecutionResult;
}
