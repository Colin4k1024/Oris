use std::sync::Arc;
use tokio::sync::Mutex;

use crate::{
    agent::AgentState,
    prompt::PromptArgs,
    tools::{StreamWriter, ToolContext, ToolStore},
};

/// Runtime information available to middleware and tools.
///
/// This matches LangChain's Runtime structure, providing access to:
/// - Context: static information like user IDs, database connections
/// - Store: BaseStore instance for long-term memory
/// - Stream writer: object for streaming information
pub struct Runtime {
    /// Immutable context (user IDs, session details, configuration)
    pub context: Arc<dyn ToolContext>,
    /// Persistent store for long-term memory
    pub store: Arc<dyn ToolStore>,
    /// Optional stream writer for real-time updates
    pub stream_writer: Option<Arc<dyn StreamWriter>>,
}

impl Runtime {
    /// Create a new Runtime
    pub fn new(context: Arc<dyn ToolContext>, store: Arc<dyn ToolStore>) -> Self {
        Self {
            context,
            store,
            stream_writer: None,
        }
    }

    /// Create with stream writer
    pub fn with_stream_writer(mut self, stream_writer: Arc<dyn StreamWriter>) -> Self {
        self.stream_writer = Some(stream_writer);
        self
    }

    /// Get a reference to the context
    pub fn context(&self) -> &dyn ToolContext {
        self.context.as_ref()
    }

    /// Get a reference to the store
    pub fn store(&self) -> &dyn ToolStore {
        self.store.as_ref()
    }

    /// Get a reference to the stream writer
    pub fn stream_writer(&self) -> Option<&Arc<dyn StreamWriter>> {
        self.stream_writer.as_ref()
    }
}

/// Request wrapper that includes runtime information for middleware.
///
/// This allows middleware to access runtime context, store, and stream writer
/// when processing requests.
pub struct RuntimeRequest {
    /// Input prompt arguments
    pub input: PromptArgs,
    /// Agent state
    pub state: Arc<Mutex<AgentState>>,
    /// Optional runtime information
    pub runtime: Option<Arc<Runtime>>,
}

impl RuntimeRequest {
    /// Create a new RuntimeRequest
    pub fn new(input: PromptArgs, state: Arc<Mutex<AgentState>>) -> Self {
        Self {
            input,
            state,
            runtime: None,
        }
    }

    /// Create with runtime
    pub fn with_runtime(mut self, runtime: Arc<Runtime>) -> Self {
        self.runtime = Some(runtime);
        self
    }

    /// Get a reference to the runtime
    pub fn runtime(&self) -> Option<&Arc<Runtime>> {
        self.runtime.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::{EmptyContext, InMemoryStore};

    #[test]
    fn test_runtime_creation() {
        let context = Arc::new(EmptyContext);
        let store = Arc::new(InMemoryStore::new());
        let runtime = Runtime::new(context, store);

        assert!(runtime.stream_writer().is_none());
    }

    #[test]
    fn test_runtime_request_creation() {
        let state = Arc::new(Mutex::new(AgentState::new()));
        let input = PromptArgs::new();
        let request = RuntimeRequest::new(input, state);

        assert!(request.runtime().is_none());
    }

    #[test]
    fn test_runtime_request_with_runtime() {
        let state = Arc::new(Mutex::new(AgentState::new()));
        let input = PromptArgs::new();
        let context = Arc::new(EmptyContext);
        let store = Arc::new(InMemoryStore::new());
        let runtime = Arc::new(Runtime::new(context, store));

        let request = RuntimeRequest::new(input, state).with_runtime(runtime);

        assert!(request.runtime().is_some());
    }
}
