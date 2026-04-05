//! Oris Evolution IPC Protocol
//!
//! JSON-RPC 2.0 style protocol for communication between Claude Code harness
//! and the Oris Evolution server via Unix Domain Socket.
//!
//! # Socket Address
//! `~/.claude/evolution/evolution.sock`
//!
//! # Protocol Version
//! 1.0

pub mod request;
pub mod response;
pub mod types;

pub use request::JsonRpcRequest;
pub use response::JsonRpcResponse;
pub use types::*;
