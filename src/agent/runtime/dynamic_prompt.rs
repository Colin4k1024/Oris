use std::sync::Arc;

use async_trait::async_trait;

use crate::{
    agent::{
        middleware::{Middleware, MiddlewareContext, MiddlewareError},
        runtime::RuntimeRequest,
    },
    prompt::PromptArgs,
    schemas::agent::AgentAction,
    tools::ToolContext,
};

/// Middleware that dynamically generates system prompts based on runtime context.
///
/// This allows you to customize the agent's behavior based on user context,
/// such as addressing users by name or adapting to their preferences.
pub struct DynamicPromptMiddleware {
    /// Function that generates the dynamic prompt based on context
    prompt_generator: Arc<dyn Fn(&dyn ToolContext) -> String + Send + Sync>,
}

impl DynamicPromptMiddleware {
    /// Create a new DynamicPromptMiddleware with a prompt generator function
    pub fn new<F>(generator: F) -> Self
    where
        F: Fn(&dyn ToolContext) -> String + Send + Sync + 'static,
    {
        Self {
            prompt_generator: Arc::new(generator),
        }
    }

    /// Create with a simple template-based generator
    ///
    /// The template can use placeholders like {user_id} or {user_name}
    /// that will be replaced with values from the context.
    pub fn with_template(template: String) -> Self {
        Self::new(move |ctx: &dyn ToolContext| {
            let mut prompt = template.clone();

            // Replace common placeholders
            if let Some(user_id) = ctx.user_id() {
                prompt = prompt.replace("{user_id}", user_id);
            }

            if let Some(session_id) = ctx.session_id() {
                prompt = prompt.replace("{session_id}", session_id);
            }

            // Try to get user_name from context
            if let Some(user_name) = ctx.get("user_name") {
                prompt = prompt.replace("{user_name}", user_name);
            }

            prompt
        })
    }
}

#[async_trait]
impl Middleware for DynamicPromptMiddleware {
    async fn before_agent_plan_with_runtime(
        &self,
        request: &RuntimeRequest,
        _steps: &[(AgentAction, String)],
        _context: &mut MiddlewareContext,
    ) -> Result<Option<PromptArgs>, MiddlewareError> {
        // Get runtime context
        let runtime = request.runtime.as_ref();
        if let Some(runtime) = runtime {
            // Generate dynamic prompt based on context
            let dynamic_prompt = (self.prompt_generator)(runtime.context());

            // Modify the input to include the dynamic prompt
            let mut modified_input = request.input.clone();

            // Try to update system message or add it
            // This depends on how the prompt is structured in the input
            if let Some(_chat_history) = modified_input.get_mut("chat_history") {
                // If chat_history exists, try to find and update system message
                // For now, we'll add it as a custom field
                modified_input.insert(
                    "dynamic_system_prompt".to_string(),
                    serde_json::json!(dynamic_prompt),
                );
            } else {
                // Add as system prompt
                modified_input.insert(
                    "system_prompt".to_string(),
                    serde_json::json!(dynamic_prompt),
                );
            }

            Ok(Some(modified_input))
        } else {
            // No runtime, use original input
            Ok(None)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::agent::Runtime;
    use crate::tools::SimpleContext;

    #[tokio::test]
    async fn test_dynamic_prompt_middleware() {
        let middleware = DynamicPromptMiddleware::new(|ctx: &dyn ToolContext| {
            let user_id = ctx.user_id().unwrap_or("unknown");
            format!("You are a helpful assistant for user: {}", user_id)
        });

        let context = Arc::new(SimpleContext::new().with_user_id("user123".to_string()));
        let runtime = Arc::new(Runtime::new(
            context.clone(),
            Arc::new(crate::tools::InMemoryStore::new()),
        ));

        let state = Arc::new(tokio::sync::Mutex::new(crate::agent::AgentState::new()));
        let mut input = PromptArgs::new();
        input.insert("input".to_string(), serde_json::json!("test"));

        let request = RuntimeRequest::new(input, state).with_runtime(runtime);

        let mut middleware_context = MiddlewareContext::new();
        let result = middleware
            .before_agent_plan_with_runtime(&request, &[], &mut middleware_context)
            .await;

        assert!(result.is_ok());
        if let Ok(Some(modified)) = result {
            assert!(
                modified.contains_key("dynamic_system_prompt")
                    || modified.contains_key("system_prompt")
            );
        }
    }

    #[test]
    fn test_dynamic_prompt_with_template() {
        let middleware = DynamicPromptMiddleware::with_template(
            "Hello {user_id}, you are a valued user.".to_string(),
        );

        let context = Arc::new(SimpleContext::new().with_user_id("user123".to_string()));
        let prompt = (middleware.prompt_generator)(context.as_ref());

        assert!(prompt.contains("user123"));
    }

    #[tokio::test]
    async fn test_dynamic_prompt_middleware_no_runtime() {
        let middleware =
            DynamicPromptMiddleware::new(|_ctx: &dyn ToolContext| "Default prompt".to_string());

        let state = Arc::new(tokio::sync::Mutex::new(crate::agent::AgentState::new()));
        let mut input = PromptArgs::new();
        input.insert("input".to_string(), serde_json::json!("test"));

        let request = RuntimeRequest::new(input, state);
        let mut middleware_context = crate::agent::middleware::MiddlewareContext::new();

        let result = middleware
            .before_agent_plan_with_runtime(&request, &[], &mut middleware_context)
            .await;

        assert!(result.is_ok());
        // Should return None when no runtime is available
        assert!(result.unwrap().is_none());
    }
}
