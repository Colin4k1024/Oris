use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::{
    agent::{AgentState, Runtime},
    language_models::{llm::LLM, GenerateResult},
    schemas::{messages::Message, StructuredOutputStrategy},
    tools::Tool,
};
use serde_json::Value;

// Note: ModelRequest cannot implement Clone because it contains trait objects
// that don't implement Clone. In practice, we'll create new instances when needed.

/// Request wrapper for model calls that allows middleware to modify
/// messages, tools, model, and response format before execution.
///
/// This matches LangChain's ModelRequest structure, providing a unified
/// interface for middleware to control what goes into each model call.
pub struct ModelRequest {
    /// Messages to send to the model
    pub messages: Vec<Message>,
    /// Available tools for the model
    pub tools: Vec<Arc<dyn Tool>>,
    /// Optional model override (if None, uses agent's default model)
    pub model: Option<Arc<dyn LLM>>,
    /// Optional response format override
    pub response_format: Option<Box<dyn StructuredOutputStrategy>>,
    /// Agent state (for accessing conversation history, custom fields)
    pub state: Arc<Mutex<AgentState>>,
    /// Runtime information (context, store, stream writer)
    pub runtime: Option<Arc<Runtime>>,
    /// Additional metadata
    pub metadata: HashMap<String, Value>,
}

impl ModelRequest {
    /// Create a new ModelRequest
    pub fn new(
        messages: Vec<Message>,
        tools: Vec<Arc<dyn Tool>>,
        state: Arc<Mutex<AgentState>>,
    ) -> Self {
        Self {
            messages,
            tools,
            model: None,
            response_format: None,
            state,
            runtime: None,
            metadata: HashMap::new(),
        }
    }

    /// Create with runtime
    pub fn with_runtime(mut self, runtime: Arc<Runtime>) -> Self {
        self.runtime = Some(runtime);
        self
    }

    /// Override messages
    pub fn override_messages(mut self, messages: Vec<Message>) -> Self {
        self.messages = messages;
        self
    }

    /// Override tools
    pub fn override_tools(mut self, tools: Vec<Arc<dyn Tool>>) -> Self {
        self.tools = tools;
        self
    }

    /// Override model
    pub fn override_model(mut self, model: Arc<dyn LLM>) -> Self {
        self.model = Some(model);
        self
    }

    /// Override response format
    pub fn override_response_format(mut self, format: Box<dyn StructuredOutputStrategy>) -> Self {
        self.response_format = Some(format);
        self
    }

    /// Add metadata
    pub fn with_metadata(mut self, key: String, value: Value) -> Self {
        self.metadata.insert(key, value);
        self
    }

    /// Get a reference to the runtime
    pub fn runtime(&self) -> Option<&Arc<Runtime>> {
        self.runtime.as_ref()
    }

    /// Get a reference to the state (requires async lock)
    pub async fn state(&self) -> tokio::sync::MutexGuard<'_, AgentState> {
        self.state.lock().await
    }

    /// Create a new ModelRequest with the same state and runtime but different messages/tools
    pub fn with_messages_and_tools(
        &self,
        messages: Vec<Message>,
        tools: Vec<Arc<dyn Tool>>,
    ) -> Self {
        Self {
            messages,
            tools,
            model: self
                .model
                .as_ref()
                .map(|_| {
                    // Note: Can't clone trait objects - return None for now
                    // In practice, models should be stored in a way that allows sharing
                    None
                })
                .flatten(),
            response_format: None, // Can't clone trait objects
            state: Arc::clone(&self.state),
            runtime: self.runtime.as_ref().map(|r| Arc::clone(r)),
            metadata: self.metadata.clone(),
        }
    }
}

/// Response wrapper for model calls that includes result and metadata.
pub struct ModelResponse {
    /// The generation result
    pub result: GenerateResult,
    /// Additional metadata about the call
    pub metadata: HashMap<String, Value>,
}

impl ModelResponse {
    /// Create a new ModelResponse
    pub fn new(result: GenerateResult) -> Self {
        Self {
            result,
            metadata: HashMap::new(),
        }
    }

    /// Create with metadata
    pub fn with_metadata(mut self, key: String, value: Value) -> Self {
        self.metadata.insert(key, value);
        self
    }
}

impl From<GenerateResult> for ModelResponse {
    fn from(result: GenerateResult) -> Self {
        Self::new(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::{EmptyContext, InMemoryStore};

    #[test]
    fn test_model_request_creation() {
        let state = Arc::new(Mutex::new(AgentState::new()));
        let messages = vec![Message::new_human_message("Hello")];
        let request = ModelRequest::new(messages, vec![], state);

        assert_eq!(request.messages.len(), 1);
        assert!(request.tools.is_empty());
        assert!(request.model.is_none());
        assert!(request.runtime().is_none());
    }

    #[test]
    fn test_model_request_override() {
        let state = Arc::new(Mutex::new(AgentState::new()));
        let messages = vec![Message::new_human_message("Hello")];
        let mut request = ModelRequest::new(messages, vec![], state);

        let new_messages = vec![
            Message::new_human_message("Hello"),
            Message::new_human_message("World"),
        ];
        request = request.override_messages(new_messages);

        assert_eq!(request.messages.len(), 2);
    }

    #[tokio::test]
    async fn test_model_request_with_runtime() {
        let state = Arc::new(Mutex::new(AgentState::new()));
        let context = Arc::new(EmptyContext);
        let store = Arc::new(InMemoryStore::new());
        let runtime = Arc::new(Runtime::new(context, store));

        let messages = vec![Message::new_human_message("Hello")];
        let request = ModelRequest::new(messages, vec![], state).with_runtime(runtime);

        assert!(request.runtime().is_some());
    }
}
