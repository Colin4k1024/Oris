use std::sync::Arc;

use async_trait::async_trait;

use crate::{
    agent::{
        context_engineering::ModelRequest,
        middleware::{Middleware, MiddlewareContext, MiddlewareError},
        AgentState,
    },
    tools::{Tool, ToolContext},
};

/// Middleware that dynamically filters and selects tools based on context.
///
/// This allows you to control which tools are available to the model
/// based on State, Store, or Runtime Context (e.g., permissions, feature flags).
pub struct DynamicToolsMiddleware {
    /// Function that filters tools based on ModelRequest
    tool_filter: Arc<dyn Fn(&ModelRequest) -> Vec<Arc<dyn Tool>> + Send + Sync>,
}

impl DynamicToolsMiddleware {
    /// Create a new DynamicToolsMiddleware with a tool filter function
    pub fn new<F>(filter: F) -> Self
    where
        F: Fn(&ModelRequest) -> Vec<Arc<dyn Tool>> + Send + Sync + 'static,
    {
        Self {
            tool_filter: Arc::new(filter),
        }
    }

    /// Create a tool filter based on State
    ///
    /// The filter function receives the AgentState and returns a list of tool names to include.
    pub fn from_state<F>(filter: F) -> Self
    where
        F: Fn(&AgentState) -> Vec<String> + Send + Sync + 'static,
    {
        Self::new(move |request: &ModelRequest| {
            if let Ok(handle) = tokio::runtime::Handle::try_current() {
                let state = handle.block_on(request.state());
                let allowed_tool_names = filter(&state);

                // Filter tools by name
                request
                    .tools
                    .iter()
                    .filter(|tool| allowed_tool_names.contains(&tool.name()))
                    .cloned()
                    .collect()
            } else {
                // Fallback: return all tools
                request.tools.clone()
            }
        })
    }

    /// Create a tool filter based on permissions from Runtime Context
    pub fn from_permissions<F>(filter: F) -> Self
    where
        F: Fn(&dyn ToolContext) -> Vec<String> + Send + Sync + 'static,
    {
        Self::new(move |request: &ModelRequest| {
            if let Some(runtime) = request.runtime() {
                let allowed_tool_names = filter(runtime.context());

                // Filter tools by name
                request
                    .tools
                    .iter()
                    .filter(|tool| allowed_tool_names.contains(&tool.name()))
                    .cloned()
                    .collect()
            } else {
                // Fallback: return all tools
                request.tools.clone()
            }
        })
    }

    /// Create a simple filter that only allows tools with specific prefixes
    pub fn allow_prefixes(prefixes: Vec<String>) -> Self {
        Self::new(move |request: &ModelRequest| {
            request
                .tools
                .iter()
                .filter(|tool| {
                    prefixes
                        .iter()
                        .any(|prefix| tool.name().starts_with(prefix))
                })
                .cloned()
                .collect()
        })
    }

    /// Create a filter that excludes tools with specific names
    pub fn exclude_tools(excluded: Vec<String>) -> Self {
        Self::new(move |request: &ModelRequest| {
            request
                .tools
                .iter()
                .filter(|tool| !excluded.contains(&tool.name()))
                .cloned()
                .collect()
        })
    }
}

#[async_trait]
impl Middleware for DynamicToolsMiddleware {
    async fn before_model_call(
        &self,
        request: &ModelRequest,
        _context: &mut MiddlewareContext,
    ) -> Result<Option<ModelRequest>, MiddlewareError> {
        // Filter tools
        let filtered_tools = (self.tool_filter)(request);

        // Only modify if tools were actually filtered
        if filtered_tools.len() != request.tools.len() {
            Ok(Some(request.with_messages_and_tools(
                request.messages.clone(),
                filtered_tools,
            )))
        } else {
            Ok(None)
        }
    }
}

impl Clone for DynamicToolsMiddleware {
    fn clone(&self) -> Self {
        Self {
            tool_filter: Arc::clone(&self.tool_filter),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schemas::Message;
    use crate::tools::SimpleContext;

    #[tokio::test]
    async fn test_dynamic_tools_exclude() {
        // This test would require actual tool instances
        // For now, we'll just test the structure
        let middleware = DynamicToolsMiddleware::exclude_tools(vec![
            "delete_tool".to_string(),
            "admin_tool".to_string(),
        ]);

        let state = Arc::new(tokio::sync::Mutex::new(AgentState::new()));
        let messages = vec![Message::new_human_message("Hello")];
        let request = ModelRequest::new(messages, vec![], state);

        let mut middleware_context = MiddlewareContext::new();
        let result = middleware
            .before_model_call(&request, &mut middleware_context)
            .await;

        assert!(result.is_ok());
    }
}
