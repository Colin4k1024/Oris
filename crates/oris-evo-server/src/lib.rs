//! Oris Evolution Server
//!
//! IPC server that provides evolution capabilities to Claude Code harness
//! via Unix Domain Socket.
//!
//! # Architecture
//!
//! ```text
//! Claude Code Harness
//!        │
//!        │ Unix Socket (JSON-RPC 2.0)
//!        ▼
//! ┌─────────────────┐
//! │  Evo Server     │
//! │  - Request      │
//! │    Handler      │
//! └────────┬────────┘
//!          │
//!          ▼
//! ┌─────────────────┐
//! │  Pipeline       │
//! │  Driver         │
//! └────────┬────────┘
//!          │
//!          ▼
//! ┌─────────────────┐
//! │  Gene Pool      │
//! │  (SQLite)       │
//! └─────────────────┘
//! ```

pub mod error;
pub mod handlers;
pub mod pipeline;
pub mod server;

pub use error::{Error, Result};
pub use server::EvoServer;
