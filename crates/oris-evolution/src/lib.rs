//! Evolution domain model, append-only event store, projections, and selector logic.

pub mod confidence;
mod core;
pub mod evolver;
pub mod gep;
pub mod pipeline;
pub mod port;
pub mod task_class;

pub use confidence::*;
pub use core::*;
pub use evolver::*;
pub use pipeline::*;
pub use port::*;
pub use task_class::{
    builtin_task_class_definitions, builtin_task_classes, load_task_classes, signals_match_class,
    TaskClass, TaskClassDefinition, TaskClassInferencer, TaskClassMatcher,
};
