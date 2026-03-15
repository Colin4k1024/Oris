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
    EvaluateInput, EvaluatePort, EvaluationRecommendation, EvaluationResult, EvolutionSignal,
    GeneStorePersistPort, PreparedMutation, SandboxExecutionResult, SandboxPort,
    SignalExtractorInput, SignalExtractorPort, SignalType, ValidateInput, ValidatePort,
    ValidationResult,
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

// ─────────────────────────────────────────────────────────────────────────────
// SqliteGeneStorePersistAdapter
// ─────────────────────────────────────────────────────────────────────────────

/// Implements `GeneStorePersistPort` using `oris_genestore::SqliteGeneStore`.
///
/// Convert `oris-evolution`'s string-typed Gene fields (signals/strategy/
/// validation) into the richer `oris_genestore::Gene` domain model, then
/// upsert into the SQLite store. The `gene_id` string is parsed as a UUID;
/// if it is not a valid UUID a new random one is generated.
///
/// Async store calls are bridged synchronously via a dedicated thread,
/// matching the pattern used by `LocalSandboxAdapter`.
pub struct SqliteGeneStorePersistAdapter {
    store: oris_genestore::SqliteGeneStore,
}

impl SqliteGeneStorePersistAdapter {
    /// Open (or create) the store at `path`. Use `":memory:"` in tests.
    pub fn open(path: &str) -> anyhow::Result<Self> {
        Ok(Self {
            store: oris_genestore::SqliteGeneStore::open(path)?,
        })
    }
}

impl GeneStorePersistPort for SqliteGeneStorePersistAdapter {
    fn persist_gene(
        &self,
        gene_id: &str,
        signals: &[String],
        strategy: &[String],
        validation: &[String],
    ) -> bool {
        use chrono::Utc;
        use oris_genestore::{Gene, GeneStore};
        use uuid::Uuid;

        let id = Uuid::parse_str(gene_id).unwrap_or_else(|_| Uuid::new_v4());
        let gene = Gene {
            id,
            name: format!("gene-{}", &gene_id[..gene_id.len().min(8)]),
            description: signals.first().cloned().unwrap_or_default(),
            tags: signals.to_vec(),
            template: strategy.join("\n"),
            preconditions: vec![],
            validation_steps: validation.to_vec(),
            confidence: 0.70,
            use_count: 0,
            success_count: 0,
            quality_score: 0.60,
            created_at: Utc::now(),
            last_used_at: None,
            last_boosted_at: None,
        };

        let store = &self.store;
        let result = match tokio::runtime::Handle::try_current() {
            Ok(handle) => std::thread::scope(|s| {
                s.spawn(|| handle.block_on(store.upsert_gene(&gene)))
                    .join()
                    .unwrap_or_else(|_| Err(anyhow::anyhow!("thread panicked")))
            }),
            Err(_) => tokio::runtime::Runtime::new()
                .map_err(|e| anyhow::anyhow!(e))
                .and_then(|rt| rt.block_on(store.upsert_gene(&gene))),
        };

        if let Err(ref e) = result {
            eprintln!("[SqliteGeneStorePersistAdapter] persist_gene error: {e}");
        }
        result.is_ok()
    }

    fn mark_reused(&self, gene_id: &str, capsule_ids: &[String]) -> bool {
        use oris_genestore::GeneStore;
        use uuid::Uuid;

        let id = match Uuid::parse_str(gene_id) {
            Ok(u) => u,
            Err(_) => return false,
        };

        let store = &self.store;
        let result = match tokio::runtime::Handle::try_current() {
            Ok(handle) => std::thread::scope(|s| {
                s.spawn(|| handle.block_on(store.record_gene_outcome(id, true)))
                    .join()
                    .unwrap_or_else(|_| Err(anyhow::anyhow!("thread panicked")))
            }),
            Err(_) => tokio::runtime::Runtime::new()
                .map_err(|e| anyhow::anyhow!(e))
                .and_then(|rt| rt.block_on(store.record_gene_outcome(id, true))),
        };

        // Log capsule IDs for traceability (store doesn't have capsule-level
        // reuse tracking in this minimal integration path).
        if !capsule_ids.is_empty() {
            eprintln!(
                "[SqliteGeneStorePersistAdapter] mark_reused gene={} capsules={:?}",
                gene_id, capsule_ids
            );
        }

        if let Err(ref e) = result {
            eprintln!("[SqliteGeneStorePersistAdapter] mark_reused error: {e}");
        }
        result.is_ok()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// SandboxOutputValidateAdapter
// ─────────────────────────────────────────────────────────────────────────────

/// Implements `ValidatePort` by parsing sandbox execution results.
///
/// Determines validation pass/fail based on:
/// - The execution success flag
/// - Presence of error keywords in stderr (e.g. `error[`, `panicked`, `FAILED`)
///
/// This adapter is pure and synchronous — no I/O.
pub struct SandboxOutputValidateAdapter;

impl SandboxOutputValidateAdapter {
    pub fn new() -> Self {
        Self
    }
}

impl Default for SandboxOutputValidateAdapter {
    fn default() -> Self {
        Self::new()
    }
}

/// Keywords in stderr that indicate a validation failure even if the process
/// exited successfully.
const FAILURE_KEYWORDS: &[&str] = &[
    "error[",
    "panicked at",
    "FAILED",
    "thread 'main' panicked",
    "cannot find",
    "aborting due to",
];

impl ValidatePort for SandboxOutputValidateAdapter {
    fn validate(&self, input: &ValidateInput) -> ValidationResult {
        let stderr_has_errors = FAILURE_KEYWORDS
            .iter()
            .any(|kw| input.stderr.contains(kw));

        let passed = input.execution_success && !stderr_has_errors;

        let mut issues = Vec::new();
        if !input.execution_success {
            issues.push(oris_evolution::ValidationIssue {
                severity: oris_evolution::IssueSeverity::Error,
                description: "Sandbox execution did not succeed".to_string(),
                location: None,
            });
        }
        if stderr_has_errors {
            issues.push(oris_evolution::ValidationIssue {
                severity: oris_evolution::IssueSeverity::Error,
                description: format!(
                    "Stderr contains error keywords: {}",
                    &input.stderr[..input.stderr.len().min(200)]
                ),
                location: None,
            });
        }

        let score = if passed { 0.9 } else { 0.1 };

        ValidationResult {
            proposal_id: input.proposal_id.clone(),
            passed,
            score,
            issues,
            simulation_results: None,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// MutationEvaluatorAdapter
// ─────────────────────────────────────────────────────────────────────────────

/// Implements `EvaluatePort` by bridging to `oris_mutation_evaluator::MutationEvaluator`.
///
/// The underlying evaluator is async; this adapter bridges the gap using the
/// same `Handle::try_current()` + thread-scoped `block_on` pattern used by
/// `LocalSandboxAdapter` and `SqliteGeneStorePersistAdapter`.
pub struct MutationEvaluatorAdapter {
    inner: oris_mutation_evaluator::MutationEvaluator,
}

impl MutationEvaluatorAdapter {
    /// Create a new adapter wrapping the provided evaluator.
    pub fn new(evaluator: oris_mutation_evaluator::MutationEvaluator) -> Self {
        Self { inner: evaluator }
    }
}

impl EvaluatePort for MutationEvaluatorAdapter {
    fn evaluate(&self, input: &EvaluateInput) -> EvaluationResult {
        use oris_mutation_evaluator::types::{EvoSignal, MutationProposal, SignalKind, Verdict};

        let proposal = MutationProposal {
            id: uuid::Uuid::new_v4(),
            intent: input.intent.clone(),
            original: input.original.clone(),
            proposed: input.proposed.clone(),
            signals: input
                .signals
                .iter()
                .map(|s| EvoSignal {
                    kind: SignalKind::Custom(s.clone()),
                    message: s.clone(),
                    location: None,
                })
                .collect(),
            source_gene_id: None,
        };

        let evaluator = &self.inner;
        let result = match tokio::runtime::Handle::try_current() {
            Ok(handle) => std::thread::scope(|s| {
                s.spawn(|| handle.block_on(evaluator.evaluate(&proposal)))
                    .join()
                    .unwrap_or_else(|_| Err(anyhow::anyhow!("evaluator thread panicked")))
            }),
            Err(_) => tokio::runtime::Runtime::new()
                .map_err(|e| anyhow::anyhow!(e))
                .and_then(|rt| rt.block_on(evaluator.evaluate(&proposal))),
        };

        match result {
            Ok(report) => {
                let recommendation = match report.verdict {
                    Verdict::Promote => EvaluationRecommendation::Accept,
                    Verdict::ApplyOnly => EvaluationRecommendation::NeedsRevision,
                    Verdict::Reject => EvaluationRecommendation::Reject,
                };
                let improvements: Vec<String> = if report.composite_score >= 0.5 {
                    vec![report.rationale.clone()]
                } else {
                    vec![]
                };
                let regressions: Vec<String> = report
                    .anti_patterns
                    .iter()
                    .map(|ap| ap.description.clone())
                    .collect();

                EvaluationResult {
                    score: report.composite_score as f32,
                    improvements,
                    regressions,
                    recommendation,
                }
            }
            Err(e) => {
                eprintln!("[MutationEvaluatorAdapter] evaluation error: {e}");
                EvaluationResult {
                    score: 0.0,
                    improvements: vec![],
                    regressions: vec![format!("Evaluation failed: {e}")],
                    recommendation: EvaluationRecommendation::RequiresHumanReview,
                }
            }
        }
    }
}
