//! oris-mutation-evaluator
//!
//! Mutation quality evaluator with static analysis and LLM critic.
//! Two-stage gate: static anti-pattern detection + semantic scoring.

pub mod critic;
pub mod evaluator;
pub mod mutation_backend;
pub mod static_analysis;
pub mod types;

pub use critic::{LlmCritic, MockCritic};
pub use evaluator::MutationEvaluator;
pub use mutation_backend::{
    ContractViolation, EnvRoutedBackend, LlmMutationBackend, MockMutationBackend, MutationRequest,
    ProposalContract,
};
pub use types::{EvaluationReport, MutationProposal, Verdict, APPLY_THRESHOLD, PROMOTE_THRESHOLD};
