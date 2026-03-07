//! Evolution domain model, append-only event store, projections, and selector logic.

mod core;
pub mod evolver;

pub use core::*;
pub use evolver::*;
