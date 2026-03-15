//! oris-genestore
//!
//! SQLite-based Gene and Capsule storage for Oris Evolution.
//!
//! Why SQLite instead of JSONL?
//! - Indexed reads: O(log n) by id, tag, confidence — vs O(n) JSONL scan
//! - Atomic writes: WAL mode prevents corruption on crash
//! - Concurrent readers: multiple Oris worker processes can read simultaneously
//! - Schema migrations: ALTER TABLE is far safer than rewriting JSONL files
//! - Aggregate queries: confidence stats, success-rate histograms — free with SQL

pub mod migrate;
pub mod store;
pub mod types;

pub use store::{GeneStore, SqliteGeneStore};
pub use types::{Capsule, Gene, GeneMatch, GeneQuery};
