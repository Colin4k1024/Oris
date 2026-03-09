//! oris-mutation-evaluator/src/evaluator.rs
//!
//! `MutationEvaluator` — the public entry-point.
//!
//! Pipeline:
//!   1. Static anti-pattern detection (instant, no I/O)
//!   2. Early rejection if blocking anti-pattern found
//!   3. LLM critic call (async, pluggable backend)
//!   4. Score aggregation → Verdict

use crate::{
    critic::LlmCritic,
    static_analysis::detect_anti_patterns,
    types::{
        AntiPatternKind, EvaluationReport, MutationProposal, Verdict,
        APPLY_THRESHOLD, PROMOTE_THRESHOLD,
    },
};
use anyhow::Result;
use chrono::Utc;
use std::sync::Arc;
use tracing::{debug, info, warn};

pub struct MutationEvaluator {
    critic: Arc<dyn LlmCritic>,
}

impl MutationEvaluator {
    pub fn new(critic: impl LlmCritic + 'static) -> Self {
        Self {
            critic: Arc::new(critic),
        }
    }

    /// Evaluate a mutation proposal and return a full `EvaluationReport`.
    pub async fn evaluate(&self, proposal: &MutationProposal) -> Result<EvaluationReport> {
        info!(proposal_id = %proposal.id, "Starting mutation evaluation");

        // ── Stage 1: static analysis ──────────────────────────────────────────
        let anti_patterns = detect_anti_patterns(proposal);
        let blocking = anti_patterns.iter().any(|a| a.is_blocking);

        if blocking {
            let reason = anti_patterns
                .iter()
                .filter(|a| a.is_blocking)
                .map(|a| a.description.as_str())
                .collect::<Vec<_>>()
                .join("; ");

            warn!(
                proposal_id = %proposal.id,
                reason = %reason,
                "Blocking anti-pattern detected — skipping LLM critic"
            );

            return Ok(EvaluationReport {
                proposal_id:     proposal.id,
                evaluated_at:    Utc::now(),
                composite_score: 0.0,
                dimensions: crate::types::DimensionScores {
                    signal_alignment:     0.0,
                    semantic_correctness: 0.0,
                    generalisability:     0.0,
                    test_coverage_delta:  0.0,
                    complexity_impact:    0.0,
                },
                verdict: Verdict::Reject,
                rationale: format!("Blocked by static analysis: {}", reason),
                anti_patterns,
            });
        }

        // ── Stage 2: LLM critic ───────────────────────────────────────────────
        debug!(proposal_id = %proposal.id, "Calling LLM critic");
        let dimensions = self.critic.evaluate(proposal).await?;
        let composite = dimensions.composite();

        // ── Stage 3: Verdict ──────────────────────────────────────────────────
        // Even without a blocking static pattern, a non-blocking anti-pattern
        // (e.g. error suppression) can drop the verdict from Promote → ApplyOnly.
        let has_soft_anti_pattern = anti_patterns.iter().any(|a| {
            matches!(
                a.kind,
                AntiPatternKind::ErrorSuppression | AntiPatternKind::BlastRadiusViolation
            )
        });

        let verdict = if composite >= PROMOTE_THRESHOLD && !has_soft_anti_pattern {
            Verdict::Promote
        } else if composite >= APPLY_THRESHOLD {
            Verdict::ApplyOnly
        } else {
            Verdict::Reject
        };

        let rationale = build_rationale(&dimensions, composite, &verdict, &anti_patterns);

        info!(
            proposal_id = %proposal.id,
            composite   = %composite,
            verdict     = ?verdict,
            "Evaluation complete"
        );

        Ok(EvaluationReport {
            proposal_id: proposal.id,
            evaluated_at: Utc::now(),
            composite_score: composite,
            dimensions,
            verdict,
            rationale,
            anti_patterns,
        })
    }
}

fn build_rationale(
    dims: &crate::types::DimensionScores,
    composite: f64,
    verdict: &Verdict,
    anti_patterns: &[crate::types::AntiPattern],
) -> String {
    let verdict_str = match verdict {
        Verdict::Promote   => "Promote",
        Verdict::ApplyOnly => "Apply-only",
        Verdict::Reject    => "Reject",
    };

    let ap_summary = if anti_patterns.is_empty() {
        "No anti-patterns detected.".to_string()
    } else {
        format!(
            "Anti-patterns: {}",
            anti_patterns
                .iter()
                .map(|a| format!("[{:?}]", a.kind))
                .collect::<Vec<_>>()
                .join(", ")
        )
    };

    format!(
        "{verdict_str} | composite={composite:.2} \
         (signal={sa:.2}, correctness={sc:.2}, general={g:.2}, test={t:.2}, complexity={c:.2}) | {ap}",
        verdict_str = verdict_str,
        composite   = composite,
        sa          = dims.signal_alignment,
        sc          = dims.semantic_correctness,
        g           = dims.generalisability,
        t           = dims.test_coverage_delta,
        c           = dims.complexity_impact,
        ap          = ap_summary,
    )
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        critic::MockCritic,
        types::{EvoSignal, SignalKind},
    };
    use uuid::Uuid;

    fn proposal(original: &str, proposed: &str) -> MutationProposal {
        MutationProposal {
            id:             Uuid::new_v4(),
            intent:         "fix type mismatch".into(),
            original:       original.to_string(),
            proposed:       proposed.to_string(),
            signals:        vec![EvoSignal {
                kind:     SignalKind::CompilerError,
                message:  "mismatched types".into(),
                location: Some("src/lib.rs:10".into()),
            }],
            source_gene_id: None,
        }
    }

    #[tokio::test]
    async fn promotes_clean_mutation() {
        let evaluator = MutationEvaluator::new(MockCritic::passing());
        let p = proposal(
            "fn add(a: i32, b: i32) -> i32 { a }",
            "fn add(a: i32, b: i32) -> i32 { a + b }",
        );
        let report = evaluator.evaluate(&p).await.unwrap();
        assert_eq!(report.verdict, Verdict::Promote);
    }

    #[tokio::test]
    async fn rejects_no_op_before_llm_call() {
        // MockCritic::passing() would promote — but static analysis must short-circuit.
        let evaluator = MutationEvaluator::new(MockCritic::passing());
        let p = proposal("fn foo() {}", "fn foo() {}");
        let report = evaluator.evaluate(&p).await.unwrap();
        assert_eq!(report.verdict, Verdict::Reject);
        assert_eq!(report.composite_score, 0.0); // never reached LLM
    }

    #[tokio::test]
    async fn rejects_low_llm_scores() {
        let evaluator = MutationEvaluator::new(MockCritic::failing());
        let p = proposal(
            "fn add(a: i32, b: i32) -> i32 { a }",
            "fn add(a: i32, b: i32) -> i32 { a + b }",
        );
        let report = evaluator.evaluate(&p).await.unwrap();
        assert_eq!(report.verdict, Verdict::Reject);
    }
}
