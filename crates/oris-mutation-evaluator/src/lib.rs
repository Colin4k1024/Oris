//! oris-mutation-evaluator
//!
//! Mutation quality evaluator with static analysis and LLM critic.
//! Two-stage gate: static anti-pattern detection + semantic scoring.

pub mod critic;
pub mod evaluator;
pub mod static_analysis;
pub mod types;

pub use evaluator::MutationEvaluator;
pub use types::{EvaluationReport, MutationProposal, Verdict, APPLY_THRESHOLD, PROMOTE_THRESHOLD};
