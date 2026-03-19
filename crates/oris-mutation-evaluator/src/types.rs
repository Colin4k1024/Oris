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

/// Composite score with time-decay and statistical confidence interval.
///
/// `raw` is the unweighted composite from `DimensionScores::composite()`.
/// `time_decayed` applies an exponential time-decay weight `w_t = exp(-λ * age_days)`
/// where `λ = 0.05` (≈ 14-day half-life).
/// `confidence_interval` is a Wilson score interval (lower, upper); the upper
/// bound is used as a pessimistic estimate when `sample_count < 10`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompositeScore {
    /// Raw weighted composite score in [0.0, 1.0].
    pub raw: f64,
    /// Time-decay-weighted score in [0.0, 1.0].
    pub time_decayed: f64,
    /// Wilson score interval `(lower, upper)` in [0.0, 1.0].
    pub confidence_interval: (f64, f64),
    /// Number of historical observations used (0 = first evaluation).
    pub sample_count: usize,
}

impl CompositeScore {
    /// Compute from a raw score, observation age in days, and historical sample count.
    ///
    /// * `raw` — result of `DimensionScores::composite()`
    /// * `age_days` — calendar age of the most-recent matching observation
    /// * `sample_count` — number of historical outcomes (0 for brand-new mutations)
    pub fn compute(raw: f64, age_days: f64, sample_count: usize) -> Self {
        const LAMBDA: f64 = 0.05;
        let weight = (-LAMBDA * age_days).exp();
        let time_decayed = (raw * weight).clamp(0.0, 1.0);
        let ci = wilson_interval(raw, sample_count);
        Self {
            raw,
            time_decayed,
            confidence_interval: ci,
            sample_count,
        }
    }

    /// Pessimistic estimate: upper bound of the Wilson CI when `sample_count < 10`,
    /// otherwise the raw score.
    pub fn pessimistic(&self) -> f64 {
        if self.sample_count < 10 {
            self.confidence_interval.1
        } else {
            self.raw
        }
    }
}

/// Wilson score interval for a proportion `p` estimated from `n` observations.
///
/// Uses a 95% confidence level (`z = 1.96`).  Returns `(0.0, 1.0)` when `n == 0`.
pub fn wilson_interval(p: f64, n: usize) -> (f64, f64) {
    if n == 0 {
        return (0.0, 1.0);
    }
    let n_f = n as f64;
    const Z: f64 = 1.96;
    let z2 = Z * Z;
    let denom = 1.0 + z2 / n_f;
    let center = (p + z2 / (2.0 * n_f)) / denom;
    let half = (Z / denom) * (p * (1.0 - p) / n_f + z2 / (4.0 * n_f * n_f)).sqrt();
    (
        (center - half).clamp(0.0, 1.0),
        (center + half).clamp(0.0, 1.0),
    )
}

// ─────────────────────────────────────────────────────────────────────────────
// Thresholds
// ─────────────────────────────────────────────────────────────────────────────

pub const PROMOTE_THRESHOLD: f64 = 0.72;
pub const APPLY_THRESHOLD: f64 = 0.45;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn time_decay_older_score_lower() {
        let fresh = CompositeScore::compute(0.8, 1.0, 20);
        let stale = CompositeScore::compute(0.8, 30.0, 20);
        assert!(
            stale.time_decayed < fresh.time_decayed,
            "30-day score ({}) should be below 1-day score ({})",
            stale.time_decayed,
            fresh.time_decayed
        );
    }

    #[test]
    fn wilson_upper_bound_below_one_for_small_sample() {
        // With 4 observations and raw=0.75, upper CI bound should be < 1.0.
        let (_, upper) = wilson_interval(0.75, 4);
        assert!(
            upper < 1.0,
            "upper CI={upper} should be < 1.0 for 4 samples"
        );
    }

    #[test]
    fn wilson_zero_samples_returns_full_range() {
        let (lo, hi) = wilson_interval(0.5, 0);
        assert!((lo - 0.0).abs() < 1e-9);
        assert!((hi - 1.0).abs() < 1e-9);
    }

    #[test]
    fn composite_pessimistic_uses_upper_bound_for_small_sample() {
        let cs = CompositeScore::compute(0.6, 0.0, 4);
        let (_, upper) = wilson_interval(0.6, 4);
        assert!((cs.pessimistic() - upper).abs() < 1e-9);
    }

    #[test]
    fn composite_pessimistic_uses_raw_for_large_sample() {
        let cs = CompositeScore::compute(0.6, 0.0, 20);
        assert!((cs.pessimistic() - 0.6).abs() < 1e-9);
    }
}
