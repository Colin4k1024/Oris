//! Adapters connecting oris-evokernel concrete implementations to the
//! `SignalExtractorPort` and `SandboxPort` traits defined in `oris-evolution`.
//!
//! Inject these into `StandardEvolutionPipeline` to wire up the Detect and
//! Execute stages with real runtime infrastructure:
//!
//! ```no_run
//! use std::sync::Arc;
//! use oris_evolution::{EvolutionPipelineConfig, StandardEvolutionPipeline};
//! use oris_evokernel::adapters::{LocalSandboxAdapter, RuntimeSignalExtractorAdapter};
//! # use oris_evolution::Selector;
//! # fn build_selector() -> Arc<dyn Selector> { unimplemented!() }
//! let pipeline = StandardEvolutionPipeline::new(
//!     EvolutionPipelineConfig::default(),
//!     build_selector(),
//! )
//! .with_signal_extractor(Arc::new(RuntimeSignalExtractorAdapter::new()))
//! .with_sandbox(Arc::new(LocalSandboxAdapter::new(
//!     "run-001",
//!     "/path/to/workspace",
//!     "/tmp/oris-sandbox",
//! )));
//! ```

use std::path::PathBuf;

use oris_evolution::{
    EvolutionSignal, PreparedMutation, SandboxExecutionResult, SandboxPort, SignalExtractorInput,
    SignalExtractorPort, SignalType,
};
use oris_sandbox::{LocalProcessSandbox, Sandbox, SandboxError, SandboxPolicy};

use crate::signal_extractor::RuntimeSignalExtractor;

// ─────────────────────────────────────────────────────────────────────────────
// RuntimeSignalExtractorAdapter
// ─────────────────────────────────────────────────────────────────────────────

/// Wraps `RuntimeSignalExtractor` to implement the `SignalExtractorPort` trait.
///
/// Converts extracted `RuntimeSignal`s into `EvolutionSignal`s required by
/// `StandardEvolutionPipeline`'s Detect stage.
pub struct RuntimeSignalExtractorAdapter {
    inner: RuntimeSignalExtractor,
}

impl RuntimeSignalExtractorAdapter {
    /// Create a new adapter with a fresh `RuntimeSignalExtractor`.
    pub fn new() -> Self {
        Self {
            inner: RuntimeSignalExtractor::new(),
        }
    }
}

impl Default for RuntimeSignalExtractorAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl SignalExtractorPort for RuntimeSignalExtractorAdapter {
    fn extract(&self, input: &SignalExtractorInput) -> Vec<EvolutionSignal> {
        let runtime_signals = self.inner.extract_all(
            input.compiler_output.as_deref(),
            input.stack_trace.as_deref(),
            input.logs.as_deref(),
        );

        runtime_signals
            .into_iter()
            .map(|rs| {
                use crate::signal_extractor::RuntimeSignalType;
                let signal_type = match rs.signal_type {
                    RuntimeSignalType::PerformanceIssue => SignalType::Performance {
                        metric: "runtime".to_string(),
                        improvement_potential: rs.confidence,
                    },
                    RuntimeSignalType::ResourceExhaustion => SignalType::ResourceOptimization {
                        resource_type: "system".to_string(),
                        current_usage: rs.confidence,
                    },
                    RuntimeSignalType::CompilerDiagnostic
                    | RuntimeSignalType::RuntimePanic
                    | RuntimeSignalType::Timeout
                    | RuntimeSignalType::TestFailure
                    | RuntimeSignalType::ConfigError
                    | RuntimeSignalType::SecurityIssue
                    | RuntimeSignalType::GenericError => SignalType::ErrorPattern {
                        error_type: format!("{:?}", rs.signal_type),
                        frequency: 1,
                    },
                };

                EvolutionSignal {
                    signal_id: rs.signal_id,
                    signal_type,
                    source_task_id: String::new(),
                    confidence: rs.confidence,
                    description: rs.content,
                    metadata: rs.metadata,
                }
            })
            .collect()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// LocalSandboxAdapter
// ─────────────────────────────────────────────────────────────────────────────

/// Wraps `LocalProcessSandbox` to implement the synchronous `SandboxPort` trait.
///
/// `Sandbox::apply` is async; this adapter bridges the gap by spawning a
/// dedicated thread that blocks on the future, which is safe regardless of
/// whether the caller is inside a tokio runtime or not.
pub struct LocalSandboxAdapter {
    inner: LocalProcessSandbox,
    policy: SandboxPolicy,
}

impl LocalSandboxAdapter {
    /// Create a new adapter.
    ///
    /// - `run_id`        — unique identifier for this pipeline run
    /// - `workspace_root` — root of the workspace being mutated
    /// - `temp_root`     — directory for temporary sandbox copies
    pub fn new<S, P, Q>(run_id: S, workspace_root: P, temp_root: Q) -> Self
    where
        S: Into<String>,
        P: Into<PathBuf>,
        Q: Into<PathBuf>,
    {
        Self {
            inner: LocalProcessSandbox::new(run_id, workspace_root, temp_root),
            policy: SandboxPolicy::default(),
        }
    }

    /// Override the default `SandboxPolicy`.
    pub fn with_policy(mut self, policy: SandboxPolicy) -> Self {
        self.policy = policy;
        self
    }
}

impl SandboxPort for LocalSandboxAdapter {
    fn execute(&self, mutation: &PreparedMutation) -> SandboxExecutionResult {
        // Clone data needed inside the thread.
        let policy = self.policy.clone();

        // We need to own the sandbox and mutation for the async block. Clone
        // the mutation; wrap the sandbox in Arc so the thread can use it.
        let mutation_owned = mutation.clone();
        let inner = &self.inner;

        // Execute the async `apply` call on a dedicated blocking thread so
        // that this function can remain synchronous without deadlocking an
        // existing tokio runtime.
        let run_result: Result<_, SandboxError> = {
            match tokio::runtime::Handle::try_current() {
                Ok(handle) => {
                    // We are inside an existing tokio runtime — offload to a
                    // blocking thread pool slot to avoid blocking the executor.
                    std::thread::scope(|s| {
                        s.spawn(|| handle.block_on(inner.apply(&mutation_owned, &policy)))
                            .join()
                            .unwrap_or_else(|_| {
                                Err(SandboxError::Io("sandbox thread panicked".into()))
                            })
                    })
                }
                Err(_) => {
                    // No runtime in scope — create a temporary one.
                    tokio::runtime::Runtime::new()
                        .map_err(|e| SandboxError::Io(e.to_string()))
                        .and_then(|rt| rt.block_on(inner.apply(&mutation_owned, &policy)))
                }
            }
        };

        match run_result {
            Ok(receipt) => {
                let changed: Vec<String> = receipt
                    .changed_files
                    .iter()
                    .map(|p| p.display().to_string())
                    .collect();
                SandboxExecutionResult {
                    success: receipt.applied,
                    stdout: changed.join("\n"),
                    stderr: String::new(),
                    duration_ms: 0,
                    message: if receipt.applied {
                        "Sandbox mutation applied successfully".to_string()
                    } else {
                        "Sandbox applied but patch was not marked as applied".to_string()
                    },
                }
            }
            Err(e) => SandboxExecutionResult::failure(
                format!("{e}"),
                format!("Sandbox execution failed: {e}"),
                0,
            ),
        }
    }
}
