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
//! .with_signal_extractor(Arc::new(RuntimeSignalExtractorAdapter::default()))
//! .with_sandbox(Arc::new(LocalSandboxAdapter::new(
//!     "run-001",
//!     "/path/to/workspace",
//!     "/tmp/oris-sandbox",
//! )));
//! ```

use std::path::PathBuf;

use oris_evolution::{
    EvaluateInput, EvaluatePort, EvaluationRecommendation, EvaluationResult, EvolutionSignal,
    GeneStorePersistPort, IssueSeverity, PreparedMutation, SandboxExecutionResult, SandboxPort,
    SignalExtractorInput, SignalExtractorPort, SignalType, ValidateInput, ValidatePort,
    ValidationIssue, ValidationResult,
};
use oris_sandbox::{LocalProcessSandbox, Sandbox, SandboxError, SandboxPolicy};
use tracing;

use crate::signal_extractor::{RuntimeSignalExtractor, SignalExtractorError};
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
    pub fn new() -> Result<Self, SignalExtractorError> {
        Ok(Self {
            inner: RuntimeSignalExtractor::new()?,
        })
    }
}

impl Default for RuntimeSignalExtractorAdapter {
    fn default() -> Self {
        Self::new().expect("built-in regex patterns are valid")
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
            tracing::warn!(error = %e, "SqliteGeneStorePersistAdapter: persist_gene failed");
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
            tracing::debug!(gene_id = %gene_id, ?capsule_ids, "SqliteGeneStorePersistAdapter: mark_reused");
        }

        if let Err(ref e) = result {
            tracing::warn!(error = %e, "SqliteGeneStorePersistAdapter: mark_reused failed");
        }
        result.is_ok()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// SandboxOutputValidateAdapter
// ─────────────────────────────────────────────────────────────────────────────

/// Implements `ValidatePort` by interpreting sandbox execution output.
///
/// **Logic** (fully synchronous, no I/O):
/// * `execution_success = true`  → `passed: true, score: 0.9`
/// * `execution_success = false` and stderr matches known failure tokens
///   (e.g. `FAILED`, `error[E`, `panicked at`) → `passed: false, score: 0.0, issues = [...]`
/// * Otherwise failure → `passed: false, score: 0.2` (generic I/O error)
pub struct SandboxOutputValidateAdapter;

impl SandboxOutputValidateAdapter {
    /// Keywords that identify hard test/compile failures in stderr.
    const FAIL_TOKENS: &'static [&'static str] = &[
        "FAILED",
        "error[E",
        "error:",
        "panicked at",
        "thread '",
        "COMPILATION FAILED",
        "test result: FAILED",
    ];

    pub fn new() -> Self {
        Self
    }
}

impl Default for SandboxOutputValidateAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl ValidatePort for SandboxOutputValidateAdapter {
    fn validate(&self, input: &ValidateInput) -> ValidationResult {
        if input.execution_success {
            return ValidationResult {
                proposal_id: input.proposal_id.clone(),
                passed: true,
                score: 0.9,
                issues: vec![],
                simulation_results: None,
            };
        }

        // Execution failed — classify the failure.
        let matching_tokens: Vec<&str> = Self::FAIL_TOKENS
            .iter()
            .copied()
            .filter(|&tok| input.stderr.contains(tok) || input.stdout.contains(tok))
            .collect();

        let (score, description) = if !matching_tokens.is_empty() {
            (
                0.0_f32,
                format!(
                    "Sandbox execution failed (matched tokens: {}). stderr snippet: {}",
                    matching_tokens.join(", "),
                    &input.stderr[..input.stderr.len().min(200)],
                ),
            )
        } else {
            (
                0.2_f32,
                format!(
                    "Sandbox execution failed without a recognised pattern. stderr snippet: {}",
                    &input.stderr[..input.stderr.len().min(200)],
                ),
            )
        };

        ValidationResult {
            proposal_id: input.proposal_id.clone(),
            passed: false,
            score,
            issues: vec![ValidationIssue {
                severity: IssueSeverity::Error,
                description,
                location: None,
            }],
            simulation_results: None,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// MutationEvaluatorAdapter
// ─────────────────────────────────────────────────────────────────────────────

/// Implements `EvaluatePort` by delegating to `oris_mutation_evaluator::MutationEvaluator`.
///
/// The evaluator is async; this adapter bridges to the synchronous `EvaluatePort` contract
/// using a dedicated thread (same pattern as `LocalSandboxAdapter`), avoiding runtime nesting.
pub struct MutationEvaluatorAdapter {
    evaluator: oris_mutation_evaluator::MutationEvaluator,
}

impl MutationEvaluatorAdapter {
    /// Construct the adapter with the given evaluator.
    /// Use `MockMutationBackend` for tests or `EnvRoutedBackend` for production.
    pub fn new(evaluator: oris_mutation_evaluator::MutationEvaluator) -> Self {
        Self { evaluator }
    }

    /// Convenience constructor using a mock critic (offline / no API key).
    /// For a production evaluator with a real LLM, construct `MutationEvaluator`
    /// manually with the desired `LlmCritic` implementation.
    pub fn from_mock() -> Self {
        let backend = oris_mutation_evaluator::MockCritic::passing();
        Self::new(oris_mutation_evaluator::MutationEvaluator::new(backend))
    }
}

impl EvaluatePort for MutationEvaluatorAdapter {
    fn evaluate(&self, input: &EvaluateInput) -> EvaluationResult {
        use oris_mutation_evaluator::types::{
            EvoSignal, MutationProposal, SignalKind as EvalSignalKind,
        };
        use uuid::Uuid;

        // Map signal strings → EvoSignal (use generic CompilerError as kind).
        let signals: Vec<EvoSignal> = input
            .signals
            .iter()
            .map(|s| EvoSignal {
                kind: EvalSignalKind::CompilerError,
                message: s.clone(),
                location: None,
            })
            .collect();

        let id = Uuid::parse_str(&input.proposal_id).unwrap_or_else(|_| Uuid::new_v4());
        let proposal = MutationProposal {
            id,
            intent: input.intent.clone(),
            original: input.original.clone(),
            proposed: input.proposed.clone(),
            signals,
            source_gene_id: None,
        };

        // Bridge async → sync using a dedicated thread so we never nest runtimes.
        let evaluator = &self.evaluator;
        let report_result = match tokio::runtime::Handle::try_current() {
            Ok(handle) => std::thread::scope(|s| {
                s.spawn(|| handle.block_on(evaluator.evaluate(&proposal)))
                    .join()
                    .unwrap_or_else(|_| Err(anyhow::anyhow!("evaluator thread panicked")))
            }),
            Err(_) => tokio::runtime::Runtime::new()
                .map_err(|e| anyhow::anyhow!(e))
                .and_then(|rt| rt.block_on(evaluator.evaluate(&proposal))),
        };

        match report_result {
            Ok(report) => {
                let recommendation = match report.verdict {
                    oris_mutation_evaluator::Verdict::Reject => EvaluationRecommendation::Reject,
                    oris_mutation_evaluator::Verdict::Promote => EvaluationRecommendation::Accept,
                    oris_mutation_evaluator::Verdict::ApplyOnly => EvaluationRecommendation::Accept,
                };
                EvaluationResult {
                    score: report.composite_score as f32,
                    improvements: vec![report.rationale.clone()],
                    regressions: report
                        .anti_patterns
                        .iter()
                        .map(|ap| ap.description.clone())
                        .collect(),
                    recommendation,
                }
            }
            Err(e) => {
                tracing::warn!(error = %e, "MutationEvaluatorAdapter: evaluate failed");
                // Fail-closed: return a neutral score and require human review.
                EvaluationResult {
                    score: 0.0,
                    improvements: vec![],
                    regressions: vec![format!("Evaluator error: {e}")],
                    recommendation: EvaluationRecommendation::RequiresHumanReview,
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oris_mutation_evaluator::{MockCritic, MutationEvaluator};

    fn sample_evaluate_input() -> EvaluateInput {
        EvaluateInput {
            proposal_id: "11111111-1111-1111-1111-111111111111".to_string(),
            intent: "Fix compiler error in docs helper".to_string(),
            original: "fn helper() -> i32 { broken_call() }".to_string(),
            proposed: "fn helper() -> i32 { 42 }".to_string(),
            signals: vec!["cannot find function `broken_call` in this scope".to_string()],
        }
    }

    #[test]
    fn validate_passes_on_success_with_clean_output() {
        let adapter = SandboxOutputValidateAdapter::new();
        let result = adapter.validate(&ValidateInput {
            proposal_id: "proposal-clean".to_string(),
            execution_success: true,
            stdout: String::new(),
            stderr: String::new(),
        });

        assert!(result.passed);
        assert_eq!(result.score, 0.9);
        assert!(result.issues.is_empty());
    }

    #[test]
    fn validate_fails_on_execution_failure_with_fail_token() {
        let adapter = SandboxOutputValidateAdapter::new();
        let result = adapter.validate(&ValidateInput {
            proposal_id: "proposal-failed".to_string(),
            execution_success: false,
            stdout: "test result: FAILED. 1 passed; 2 failed".to_string(),
            stderr: "".to_string(),
        });

        assert!(!result.passed);
        assert_eq!(result.score, 0.0);
        assert_eq!(result.issues.len(), 1);
        assert!(result.issues[0].description.contains("FAILED"));
    }

    #[test]
    fn validate_passes_on_success_even_if_stdout_has_warnings() {
        let adapter = SandboxOutputValidateAdapter::new();
        let result = adapter.validate(&ValidateInput {
            proposal_id: "proposal-warn".to_string(),
            execution_success: true,
            stdout: "warning: unused import\nwarning: dead_code".to_string(),
            stderr: String::new(),
        });

        assert!(result.passed);
        assert_eq!(result.score, 0.9);
        assert!(result.issues.is_empty());
    }

    #[test]
    fn evaluator_adapter_from_mock_returns_accept() {
        let adapter = MutationEvaluatorAdapter::from_mock();
        let result = adapter.evaluate(&sample_evaluate_input());

        assert_eq!(result.recommendation, EvaluationRecommendation::Accept);
        assert!(result.score > 0.0);
        assert_eq!(result.improvements.len(), 1);
        assert!(result.regressions.is_empty());
    }

    #[test]
    fn evaluator_adapter_maps_reject_to_reject() {
        let adapter = MutationEvaluatorAdapter::new(MutationEvaluator::new(MockCritic::failing()));
        let result = adapter.evaluate(&sample_evaluate_input());

        assert_eq!(result.recommendation, EvaluationRecommendation::Reject);
        assert!(result.score >= 0.0);
    }
}
