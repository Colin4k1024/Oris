use std::sync::Arc;
use tokio::sync::Mutex;

use crate::agent::AgentState;

pub use super::context::ToolContext;
pub use super::file_backend::FileBackend;
pub use super::store::ToolStore;
pub use super::stream::StreamWriter;

/// Runtime information available to tools during execution.
///
/// Provides access to state, context, store, streaming, and other
/// runtime information. The `runtime` parameter is automatically
/// injected by the executor and is not exposed to the LLM schema.
///
/// # Long-term Memory
///
/// Tools can access long-term memory through the `store()` method.
/// If the store implements `EnhancedToolStore`, tools can use advanced
/// features like vector search and metadata filtering:
///
/// ```rust,ignore
/// use oris::tools::long_term_memory::EnhancedToolStore;
///
/// async fn my_tool(runtime: &ToolRuntime) -> Result<String, Box<dyn Error>> {
///     // Check if store supports enhanced features
///     let store = runtime.store();
///     if let Some(enhanced_store) = store.as_any().downcast_ref::<Arc<dyn EnhancedToolStore>>() {
///         // Use enhanced features like search
///         let results = enhanced_store.search(&["users"], Some("query"), None, 5).await?;
///     }
///     Ok("done".to_string())
/// }
/// ```
pub struct ToolRuntime {
    /// Mutable agent state (messages, custom fields)
    pub state: Arc<Mutex<AgentState>>,
    /// Immutable context (user IDs, session details, configuration)
    pub context: Arc<dyn ToolContext>,
    /// Persistent store for long-term memory
    ///
    /// This store can implement both `ToolStore` (basic operations)
    /// and `EnhancedToolStore` (vector search, metadata, filtering).
    pub store: Arc<dyn ToolStore>,
    /// Optional stream writer for real-time updates
    pub stream_writer: Option<Arc<dyn StreamWriter>>,
    /// Optional file backend for FS tools (ls, read_file, write_file, edit_file, glob, grep)
    pub file_backend: Option<Arc<dyn FileBackend>>,
    /// Current tool call ID
    pub tool_call_id: String,
}

impl ToolRuntime {
    pub fn new(
        state: Arc<Mutex<AgentState>>,
        context: Arc<dyn ToolContext>,
        store: Arc<dyn ToolStore>,
        tool_call_id: String,
    ) -> Self {
        Self {
            state,
            context,
            store,
            stream_writer: None,
            file_backend: None,
            tool_call_id,
        }
    }

    pub fn with_stream_writer(mut self, writer: Arc<dyn StreamWriter>) -> Self {
        self.stream_writer = Some(writer);
        self
    }

    pub fn with_file_backend(mut self, backend: Arc<dyn FileBackend>) -> Self {
        self.file_backend = Some(backend);
        self
    }

    /// File backend for FS tools; when None, tools use workspace from context.
    pub fn file_backend(&self) -> Option<&Arc<dyn FileBackend>> {
        self.file_backend.as_ref()
    }

    /// Get a reference to the state (requires async lock)
    pub async fn state(&self) -> tokio::sync::MutexGuard<'_, AgentState> {
        self.state.lock().await
    }

    /// Get a reference to the context
    pub fn context(&self) -> &dyn ToolContext {
        self.context.as_ref()
    }

    /// Get a reference to the store
    pub fn store(&self) -> &dyn ToolStore {
        self.store.as_ref()
    }

    /// Try to get the store as an EnhancedToolStore
    ///
    /// This allows tools to access enhanced features like vector search
    /// if the store supports them. Returns None if the store doesn't
    /// implement EnhancedToolStore.
    ///
    /// Note: Due to Rust's type system, this requires the store to be
    /// wrapped in a way that allows downcasting. For best results, use
    /// `EnhancedInMemoryStore` directly when creating the agent.
    pub fn enhanced_store(&self) -> Option<&dyn crate::tools::long_term_memory::EnhancedToolStore> {
        // This is a limitation - we can't easily downcast from trait object
        // Tools should check the store type or use a helper wrapper
        None
    }

    /// Write a stream update if stream writer is available
    pub fn stream(&self, message: &str) {
        if let Some(writer) = &self.stream_writer {
            writer.write(message);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::{EmptyContext, InMemoryStore};

    #[tokio::test]
    async fn test_tool_runtime_creation() {
        let state = Arc::new(Mutex::new(AgentState::new()));
        let context = Arc::new(EmptyContext);
        let store = Arc::new(InMemoryStore::new());

        let runtime = ToolRuntime::new(state, context, store, "test_call_1".to_string());

        assert_eq!(runtime.tool_call_id, "test_call_1");
    }

    #[tokio::test]
    async fn test_tool_runtime_state_access() {
        let state = Arc::new(Mutex::new(AgentState::new()));
        let context = Arc::new(EmptyContext);
        let store = Arc::new(InMemoryStore::new());

        let runtime = ToolRuntime::new(
            Arc::clone(&state),
            context,
            store,
            "test_call_1".to_string(),
        );

        {
            let mut state_guard = runtime.state().await;
            state_guard.set_field("test_key".to_string(), serde_json::json!("test_value"));
        }

        let state_guard = runtime.state().await;
        assert_eq!(
            state_guard.get_field("test_key"),
            Some(&serde_json::json!("test_value"))
        );
    }

    #[tokio::test]
    async fn test_tool_runtime_context() {
        let state = Arc::new(Mutex::new(AgentState::new()));
        let context = Arc::new(EmptyContext);
        let store = Arc::new(InMemoryStore::new());

        let runtime = ToolRuntime::new(state, context, store, "test_call_1".to_string());

        let context_ref = runtime.context();
        assert_eq!(context_ref.user_id(), None);
    }

    #[tokio::test]
    async fn test_tool_runtime_store() {
        let state = Arc::new(Mutex::new(AgentState::new()));
        let context = Arc::new(EmptyContext);
        let store: Arc<dyn ToolStore> = Arc::new(InMemoryStore::new());

        let runtime = ToolRuntime::new(
            state,
            context,
            Arc::clone(&store),
            "test_call_1".to_string(),
        );

        runtime
            .store()
            .put(&["test"], "key1", serde_json::json!("value1"))
            .await;

        let value = runtime.store().get(&["test"], "key1").await;
        assert_eq!(value, Some(serde_json::json!("value1")));
    }
}
