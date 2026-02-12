//! Pluggable file backends for deep agent filesystem tools.
//!
//! See [Backends](https://docs.langchain.com/oss/python/deepagents/backends).
//!
//! - **Default**: When no backend is set, FS tools use the workspace root from config/context (same as [WorkspaceBackend]).
//! - [WorkspaceBackend]: Local disk under a root directory (sandboxed).
//! - [StoreBackend]: Files stored in [crate::tools::ToolStore] (e.g. InMemoryStore); durable across threads.
//! - [CompositeBackend]: Routes requests by path prefix to different backends (longest-prefix match). Used for [long-term memory](https://docs.langchain.com/oss/python/deepagents/long-term-memory) by routing e.g. `/memories/` to [StoreBackend].

mod composite;
mod store;
mod workspace;

pub use crate::tools::{EditResult, FileBackend, FileInfo, GrepMatch, WriteResult};
pub use composite::CompositeBackend;
pub use store::StoreBackend;
pub use workspace::WorkspaceBackend;
