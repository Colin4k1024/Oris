//! oris-mutation-evaluator/src/types.rs
//!
//! Core domain types for the Mutation Quality Evaluator.
//! These are intentionally decoupled from the LLM transport layer so that
//! the evaluator can be driven by any backend (OpenAI, Anthropic, local Ollama …).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ─────────────────────────────────────────────────────────────────────────────
// Input
// ─────────────────────────────────────────────────────────────────────────────

/// Everything the evaluator needs to know about a proposed mutation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MutationProposal {
    /// Stable identifier that ties back to `oris-evolution`'s `MutationProposal`.
    pub id: Uuid,

    /// Human-readable description of what this mutation is trying to achieve.
    pub intent: String,

    /// The original code/prompt/configuration that is being mutated.
    pub original: String,

    /// The proposed replacement produced by the LLM mutator.
    pub proposed: String,

    /// Structured signals extracted by `oris-evokernel` (compiler errors, panics, …).
    pub signals: Vec<EvoSignal>,

    /// The Gene that was selected as the basis for this mutation, if any.
    pub source_gene_id: Option<Uuid>,
}

/// A single runtime signal that triggered the evolution cycle.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvoSignal {
    pub kind: SignalKind,
    pub message: String,
    pub location: Option<String>, // e.g. "src/main.rs:42"
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SignalKind {
    CompilerError,
    Panic,
    TestFailure,
    LintWarning,
    PerfRegression,
    Custom(String),
}

// ─────────────────────────────────────────────────────────────────────────────
// Output
// ─────────────────────────────────────────────────────────────────────────────

/// Final verdict returned to the `EvolutionPipeline`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvaluationReport {
    pub proposal_id: Uuid,
    pub evaluated_at: DateTime<Utc>,

    /// Composite quality score in [0.0, 1.0].
    pub composite_score: f64,

    /// Individual dimension scores.
    pub dimensions: DimensionScores,

    /// Whether this mutation should be promoted to a Gene.
    pub verdict: Verdict,

    /// Rich explanation surfaced to human reviewers / HITL interrupt.
    pub rationale: String,

    /// Detected anti-patterns (hardcoded values, suppressed errors, …).
    pub anti_patterns: Vec<AntiPattern>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DimensionScores {
    /// Does the mutation correctly address the triggering signal? [0,1]
    pub signal_alignment: f64,

    /// Does it avoid semantic regression (hardcode bypass, error suppression)? [0,1]
    pub semantic_correctness: f64,

    /// How well does it generalise beyond the specific failure case? [0,1]
    pub generalisability: f64,

    /// Does it introduce new test coverage? [0,1]
    pub test_coverage_delta: f64,

    /// Is complexity kept under control? [0,1]  (inverse of cyclomatic growth)
    pub complexity_impact: f64,
}

impl DimensionScores {
    /// Weighted composite — weights reflect EvoMap's GDI philosophy.
    /// quality ≈ signal_alignment + correctness, usage ≈ generalisability,
    /// recency covered externally by confidence decay.
    pub fn composite(&self) -> f64 {
        const W_SIGNAL: f64 = 0.30;
        const W_CORRECT: f64 = 0.30;
        const W_GENERAL: f64 = 0.20;
        const W_TEST: f64 = 0.10;
        const W_COMPLEX: f64 = 0.10;

        self.signal_alignment * W_SIGNAL
            + self.semantic_correctness * W_CORRECT
            + self.generalisability * W_GENERAL
            + self.test_coverage_delta * W_TEST
            + self.complexity_impact * W_COMPLEX
    }
}

/// Promotion decision with minimum thresholds.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    /// Promote to Gene — score ≥ PROMOTE_THRESHOLD and no blocking anti-patterns.
    Promote,
    /// Apply once but do not solidify as a Gene — score ≥ APPLY_THRESHOLD.
    ApplyOnly,
    /// Reject — too low quality or blocking anti-pattern detected.
    Reject,
}

/// An anti-pattern that can block promotion.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AntiPattern {
    pub kind: AntiPatternKind,
    pub description: String,
    /// If true, this anti-pattern alone is enough to reject the mutation.
    pub is_blocking: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AntiPatternKind {
    /// Hardcoded value inserted to make a specific test pass.
    HardcodedBypass,
    /// Error/panic suppressed with `unwrap_or_default`, `let _ =`, etc.
    ErrorSuppression,
    /// Test deleted or ignored rather than fixed.
    TestDeletion,
    /// Scope of the change is far larger than the signal warrants.
    BlastRadiusViolation,
    /// Mutation is byte-for-byte identical to original.
    NoOpMutation,
    Custom(String),
}

// ─────────────────────────────────────────────────────────────────────────────
// Thresholds
// ─────────────────────────────────────────────────────────────────────────────

pub const PROMOTE_THRESHOLD: f64 = 0.72;
pub const APPLY_THRESHOLD: f64 = 0.45;
