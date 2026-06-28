//! Local process sandbox for applying mutation artifacts into a temporary workspace copy.
//!
//! # Example
//!
//! ```rust,no_run
//! use oris_sandbox::LocalProcessSandbox;
//!
//! let sandbox = LocalProcessSandbox::new("run-001", "/project", "/tmp/oris-sandbox");
//! // sandbox.apply_and_validate(patch, validation_commands)
//! ```

mod core;
#[cfg(feature = "resource-limits")]
pub mod resource_limits;

pub use core::*;
