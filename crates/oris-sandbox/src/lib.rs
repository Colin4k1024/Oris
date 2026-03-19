//! Local process sandbox for applying mutation artifacts into a temporary workspace copy.

mod core;
#[cfg(feature = "resource-limits")]
pub mod resource_limits;

pub use core::*;
