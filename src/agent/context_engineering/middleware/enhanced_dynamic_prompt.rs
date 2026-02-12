use std::sync::Arc;

use async_trait::async_trait;

use crate::{
    agent::{
        context_engineering::ModelRequest,
        middleware::{Middleware, MiddlewareContext, MiddlewareError},
        AgentState,
    },
    tools::{ToolContext, ToolStore},
};

/// Enhanced dynamic prompt middleware that can access State, Store, and Runtime Context.
///
/// This extends the basic DynamicPromptMiddleware to support reading from
/// different data sources to generate context-aware prompts.
pub struct EnhancedDynamicPromptMiddleware {
    /// Function that generates the dynamic prompt based on ModelRequest
    prompt_generator: Arc<dyn Fn(&ModelRequest) -> String + Send + Sync>,
}

impl EnhancedDynamicPromptMiddleware {
    /// Create a new EnhancedDynamicPromptMiddleware with a prompt generator function
    pub fn new<F>(generator: F) -> Self
    where
        F: Fn(&ModelRequest) -> String + Send + Sync + 'static,
    {
        Self {
            prompt_generator: Arc::new(generator),
        }
    }

    /// Create a prompt generator that reads from State
    ///
    /// The generator function receives the AgentState and returns a prompt string.
    pub fn from_state<F>(generator: F) -> Self
    where
        F: Fn(&AgentState) -> String + Send + Sync + 'static,
    {
        Self::new(move |request: &ModelRequest| {
            // Note: This requires async, so we'll need to handle it differently
            // For now, we'll use a blocking approach with a runtime
            // In practice, this should be called from an async context
            if let Ok(handle) = tokio::runtime::Handle::try_current() {
                let state = handle.block_on(request.state());
                generator(&state)
            } else {
                // Fallback: use default prompt
                "You are a helpful assistant.".to_string()
            }
        })
    }

    /// Create a prompt generator that reads from Store and Context
    pub fn from_store<F>(generator: F) -> Self
    where
        F: Fn(&dyn ToolStore, &dyn ToolContext) -> String + Send + Sync + 'static,
    {
        Self::new(move |request: &ModelRequest| {
            if let Some(runtime) = request.runtime() {
                generator(runtime.store(), runtime.context())
            } else {
                "You are a helpful assistant.".to_string()
            }
        })
    }

    /// Create a prompt generator that reads from Runtime
    pub fn from_runtime<F>(generator: F) -> Self
    where
        F: Fn(&crate::agent::Runtime) -> String + Send + Sync + 'static,
    {
        Self::new(move |request: &ModelRequest| {
            if let Some(runtime) = request.runtime() {
                generator(runtime)
            } else {
                "You are a helpful assistant.".to_string()
            }
        })
    }

    /// Create with a simple template that supports State/Store placeholders
    pub fn with_template(template: String) -> Self {
        Self::new(move |request: &ModelRequest| {
            let mut prompt = template.clone();

            // Replace runtime context placeholders
            if let Some(runtime) = request.runtime() {
                if let Some(user_id) = runtime.context().user_id() {
                    prompt = prompt.replace("{user_id}", user_id);
                }

                if let Some(session_id) = runtime.context().session_id() {
                    prompt = prompt.replace("{session_id}", session_id);
                }
            }

            // Note: State placeholders would require async access
            // For template-based approach, consider using from_state for complex logic

            prompt
        })
    }
}

#[async_trait]
impl Middleware for EnhancedDynamicPromptMiddleware {
    async fn before_model_call(
        &self,
        request: &ModelRequest,
        _context: &mut MiddlewareContext,
    ) -> Result<Option<ModelRequest>, MiddlewareError> {
        // Generate dynamic prompt based on context
        let dynamic_prompt = (self.prompt_generator)(request);

        // Inject the prompt as a system message at the beginning
        let mut messages = request.messages.clone();

        // Check if there's already a system message
        let has_system = messages
            .iter()
            .any(|m| matches!(m.message_type, crate::schemas::MessageType::SystemMessage));

        if !has_system {
            // Insert system message at the beginning
            messages.insert(
                0,
                crate::schemas::Message::new_system_message(&dynamic_prompt),
            );
        } else {
            // Prepend to first system message or add new one
            if let Some(first_msg) = messages.first_mut() {
                if matches!(
                    first_msg.message_type,
                    crate::schemas::MessageType::SystemMessage
                ) {
                    // Prepend to existing system message
                    first_msg.content = format!("{}\n\n{}", dynamic_prompt, first_msg.content);
                } else {
                    messages.insert(
                        0,
                        crate::schemas::Message::new_system_message(&dynamic_prompt),
                    );
                }
            }
        }

        Ok(Some(
            request.with_messages_and_tools(messages, request.tools.clone()),
        ))
    }
}

impl Clone for EnhancedDynamicPromptMiddleware {
    fn clone(&self) -> Self {
        Self {
            prompt_generator: Arc::clone(&self.prompt_generator),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schemas::Message;
    use crate::tools::{InMemoryStore, SimpleContext};

    #[tokio::test]
    async fn test_enhanced_dynamic_prompt_from_runtime() {
        let middleware = EnhancedDynamicPromptMiddleware::from_runtime(|runtime| {
            let user_id = runtime.context().user_id().unwrap_or("unknown");
            format!("You are a helpful assistant for user: {}", user_id)
        });

        let state = Arc::new(tokio::sync::Mutex::new(AgentState::new()));
        let context = Arc::new(SimpleContext::new().with_user_id("user123".to_string()));
        let store = Arc::new(InMemoryStore::new());
        let runtime = Arc::new(crate::agent::Runtime::new(context, store));

        let messages = vec![Message::new_human_message("Hello")];
        let request = ModelRequest::new(messages, vec![], state).with_runtime(runtime);

        let mut middleware_context = MiddlewareContext::new();
        let result = middleware
            .before_model_call(&request, &mut middleware_context)
            .await;

        assert!(result.is_ok());
        if let Ok(Some(modified)) = result {
            // Should have a system message
            assert!(!modified.messages.is_empty());
            assert!(matches!(
                modified.messages[0].message_type,
                crate::schemas::MessageType::SystemMessage
            ));
        }
    }
}
