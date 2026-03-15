//! Evolution domain model, append-only event store, projections, and selector logic.

pub mod confidence;
mod core;
pub mod evolver;
pub mod gep;
pub mod pipeline;
pub mod port;

pub use confidence::*;
pub use core::*;
pub use evolver::*;
pub use pipeline::*;
pub use port::*;
