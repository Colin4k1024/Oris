//! Evolution domain model, append-only event store, projections, and selector logic.

mod core;
pub mod evolver;
pub mod gep;
pub mod pipeline;

pub use core::*;
pub use evolver::*;
pub use pipeline::*;
