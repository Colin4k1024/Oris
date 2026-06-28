#![allow(dead_code)]

//! Deterministic execution kernel for Oris.
//!
//! Provides event-sourced execution with replay, snapshot, and interrupt support.
//!
//! # Key types
//!
//! - [`Kernel`] — the execution driver with event store, reducer, and policy
//! - [`RunId`] — unique run identity
//! - [`Event`] / [`EventStore`] — append-only event log (source of truth)
//! - [`KernelMode`] — Normal, Record, Replay, or Verify
//! - [`DeterminismGuard`] — traps non-deterministic operations in replay mode
//!
//! # Example
//!
//! ```rust
//! use oris_kernel::{RunId, InMemoryEventStore, EventStore};
//!
//! let run_id = RunId::new();
//! let store = InMemoryEventStore::new();
//! // Fresh store has head at sequence 0
//! assert_eq!(store.head(&run_id).unwrap(), 0);
//! ```

pub mod kernel;

pub use kernel::*;
