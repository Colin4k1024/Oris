//! Evolution domain model, append-only event store, projections, and selector logic.

pub mod approval;
pub mod confidence;
mod core;
pub mod evolver;
pub mod gep;
pub mod pipeline;
pub mod port;
pub mod semantic_match;
pub mod task_class;

pub use approval::{
    ApprovalCheckpoint, EscalationPolicy, EscalationTrigger, Evidence, EvidenceCompleteness,
    HumanReviewRequirement,
};
pub use confidence::*;
pub use core::*;
pub use evolver::*;
pub use pipeline::*;
pub use port::*;
pub use semantic_match::{
    builtin_equivalence_classes, normalise_signal, BoundedEquivalenceClass, SemanticMatchConfig,
    SemanticMatchResult, SemanticTaskMatcher,
};
pub use task_class::{
    builtin_task_class_definitions, builtin_task_classes, load_task_classes, signals_match_class,
    TaskClass, TaskClassDefinition, TaskClassInferencer, TaskClassMatcher,
};
