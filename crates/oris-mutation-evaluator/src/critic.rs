//! oris-mutation-evaluator/src/critic.rs
//!
//! LLM-based semantic critic. Pluggable via the `LlmCritic` trait so that
//! the evaluator is not hard-wired to any provider (OpenAI, Anthropic, Ollama …).
//!
//! The critic is the second gate after static analysis. It scores the five
//! semantic dimensions that static analysis cannot reason about.

use crate::types::{DimensionScores, MutationProposal};
use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

// ─────────────────────────────────────────────────────────────────────────────
// Trait
// ─────────────────────────────────────────────────────────────────────────────

/// Implement this trait to plug in any LLM backend.
#[async_trait]
pub trait LlmCritic: Send + Sync {
    /// Send a structured prompt to the LLM and parse the JSON response.
    async fn evaluate(&self, proposal: &MutationProposal) -> Result<DimensionScores>;
}

// ─────────────────────────────────────────────────────────────────────────────
// Prompt construction
// ─────────────────────────────────────────────────────────────────────────────

/// Build the system prompt for the critic LLM.
/// Keeping it in one place means we can version-control the rubric independently.
pub fn build_system_prompt() -> &'static str {
    r#"You are a senior Rust code reviewer acting as a mutation quality critic.
Your role is to evaluate whether a proposed code mutation genuinely fixes the
reported problem or merely bypasses it.

## Output contract
Respond with a single JSON object — no markdown fences, no prose outside the object.
Schema:
{
  "signal_alignment":    <float 0.0–1.0>,  // Does the mutation address the root cause of the signal?
  "semantic_correctness": <float 0.0–1.0>, // Is the logic semantically correct (no hardcode bypass, no silent error swallowing)?
  "generalisability":    <float 0.0–1.0>,  // Will this fix work for inputs beyond the failing test case?
  "test_coverage_delta": <float 0.0–1.0>,  // 0.5 = neutral, >0.5 = improves coverage, <0.5 = reduces coverage
  "complexity_impact":   <float 0.0–1.0>,  // 1.0 = no complexity added, lower = more complex
  "rationale":           <string>          // ≤ 3 sentences explaining the scores
}

## Scoring guide
- signal_alignment: 1.0 if the mutation surgically fixes the exact error; 0.0 if unrelated.
- semantic_correctness: Deduct heavily for: hardcoded return values, `unwrap_or_default()` hiding errors, panic-suppression, test deletions.
- generalisability: Consider whether the fix handles edge cases, not just the one failing input.
- test_coverage_delta: Did the mutation add, maintain, or remove tests?
- complexity_impact: Prefer simple, readable fixes over clever ones.
"#
}

/// Build the user message for a specific proposal.
pub fn build_user_prompt(proposal: &MutationProposal) -> String {
    let signals_text = proposal
        .signals
        .iter()
        .map(|s| format!("[{:?}] {} (at {:?})", s.kind, s.message, s.location))
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        r#"## Evolution intent
{intent}

## Runtime signals that triggered this mutation
{signals}

## Original code
```rust
{original}
```

## Proposed mutation
```rust
{proposed}
```

Evaluate the mutation and return the JSON object as specified."#,
        intent = proposal.intent,
        signals = signals_text,
        original = proposal.original,
        proposed = proposal.proposed,
    )
}

// ─────────────────────────────────────────────────────────────────────────────
// LLM response parsing
// ─────────────────────────────────────────────────────────────────────────────

/// Raw JSON shape returned by the critic LLM.
#[derive(Debug, Deserialize, Serialize)]
#[allow(dead_code)]
pub(crate) struct CriticResponse {
    pub signal_alignment: f64,
    pub semantic_correctness: f64,
    pub generalisability: f64,
    pub test_coverage_delta: f64,
    pub complexity_impact: f64,
    pub rationale: String,
}

impl CriticResponse {
    #[allow(dead_code)]
    pub fn into_scores_and_rationale(self) -> (DimensionScores, String) {
        let scores = DimensionScores {
            signal_alignment: self.signal_alignment.clamp(0.0, 1.0),
            semantic_correctness: self.semantic_correctness.clamp(0.0, 1.0),
            generalisability: self.generalisability.clamp(0.0, 1.0),
            test_coverage_delta: self.test_coverage_delta.clamp(0.0, 1.0),
            complexity_impact: self.complexity_impact.clamp(0.0, 1.0),
        };
        (scores, self.rationale)
    }
}

/// Strip markdown code fences that some LLMs add despite instructions.
pub fn strip_fences(raw: &str) -> &str {
    let trimmed = raw.trim();
    if trimmed.starts_with("```") {
        // drop first line and last fence
        let inner = trimmed.split_once('\n').map(|x| x.1).unwrap_or(trimmed);
        inner.trim_end_matches("```").trim()
    } else {
        trimmed
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Built-in mock critic (for tests / offline mode)
// ─────────────────────────────────────────────────────────────────────────────

/// A deterministic mock critic that returns fixed scores.
/// Useful for unit tests and CI environments without an API key.
pub struct MockCritic {
    pub scores: DimensionScores,
}

impl MockCritic {
    pub fn passing() -> Self {
        Self {
            scores: DimensionScores {
                signal_alignment: 0.90,
                semantic_correctness: 0.85,
                generalisability: 0.80,
                test_coverage_delta: 0.60,
                complexity_impact: 0.90,
            },
        }
    }

    pub fn failing() -> Self {
        Self {
            scores: DimensionScores {
                signal_alignment: 0.20,
                semantic_correctness: 0.10,
                generalisability: 0.15,
                test_coverage_delta: 0.30,
                complexity_impact: 0.50,
            },
        }
    }
}

#[async_trait]
impl LlmCritic for MockCritic {
    async fn evaluate(&self, _proposal: &MutationProposal) -> Result<DimensionScores> {
        Ok(self.scores.clone())
    }
}
