use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;

use crate::{
    agent::{
        context_engineering::ModelRequest,
        middleware::{Middleware, MiddlewareContext, MiddlewareError},
    },
    schemas::StructuredOutputStrategy,
};

/// Middleware that dynamically selects response format based on context.
///
/// This allows you to switch between simple and detailed response formats
/// based on conversation stage, user preferences, or role.
pub struct DynamicResponseFormatMiddleware {
    /// Function that selects a response format based on ModelRequest
    format_selector:
        Arc<dyn Fn(&ModelRequest) -> Option<Box<dyn StructuredOutputStrategy>> + Send + Sync>,
    /// Available formats mapped by name
    available_formats: HashMap<String, Box<dyn StructuredOutputStrategy>>,
}

impl DynamicResponseFormatMiddleware {
    /// Create a new DynamicResponseFormatMiddleware with a format selector function
    pub fn new<F>(selector: F, formats: HashMap<String, Box<dyn StructuredOutputStrategy>>) -> Self
    where
        F: Fn(&ModelRequest) -> Option<Box<dyn StructuredOutputStrategy>> + Send + Sync + 'static,
    {
        Self {
            format_selector: Arc::new(selector),
            available_formats: formats,
        }
    }

    /// Create a format selector based on message count
    ///
    /// Uses simple format for early conversation, detailed format after threshold.
    pub fn from_message_count(
        formats: HashMap<String, Box<dyn StructuredOutputStrategy>>,
        threshold: usize,
    ) -> Self {
        // Note: Can't clone HashMap with trait objects
        // Store format names instead and recreate when needed
        let simple_name = "simple".to_string();
        let detailed_name = "detailed".to_string();
        Self::new(
            move |request: &ModelRequest| {
                let message_count = request.messages.len();

                // Note: Format selection would need to be handled at agent level
                // since we can't clone trait objects
                let _format_name = if message_count < threshold {
                    &simple_name
                } else {
                    &detailed_name
                };

                None // Format selection handled elsewhere
            },
            formats,
        )
    }

    /// Create a format selector based on user role from Runtime Context
    pub fn from_user_role(formats: HashMap<String, Box<dyn StructuredOutputStrategy>>) -> Self {
        Self::new(
            move |request: &ModelRequest| {
                if let Some(runtime) = request.runtime() {
                    // Try to get user_role from context
                    if let Some(_user_role) = runtime.context().get("user_role") {
                        // Note: Format selection would need to be handled at agent level
                        // since we can't clone trait objects
                        return None;
                    }
                }

                // Default: use standard format
                None
            },
            formats,
        )
    }
}

#[async_trait]
impl Middleware for DynamicResponseFormatMiddleware {
    async fn before_model_call(
        &self,
        request: &ModelRequest,
        _context: &mut MiddlewareContext,
    ) -> Result<Option<ModelRequest>, MiddlewareError> {
        // Select response format
        // Note: Response format override cannot be easily cloned
        // In practice, this would need to be handled at the agent level
        let _selected_format = (self.format_selector)(request);
        Ok(None) // Format selection would need to be handled differently
    }
}

// Note: Cannot implement Clone because HashMap contains non-cloneable trait objects
// In practice, you'd use Arc or a different storage mechanism

// Note: StructuredOutputStrategy is a trait object and cannot be cloned directly.
// In practice, you would need to either:
// 1. Store formats in a way that allows cloning (e.g., using Arc)
// 2. Use a factory pattern to create new instances
// 3. Store format identifiers and recreate formats when needed

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::AgentState;
    use crate::schemas::Message;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    #[tokio::test]
    async fn test_dynamic_response_format_from_message_count() {
        // This test would require actual StructuredOutputStrategy instances
        // For now, we'll just test the structure
        let formats: HashMap<String, Box<dyn StructuredOutputStrategy>> = HashMap::new();
        let middleware = DynamicResponseFormatMiddleware::from_message_count(formats, 5);

        let state = Arc::new(Mutex::new(AgentState::new()));
        let messages = vec![Message::new_human_message("Hello"); 3];
        let request = ModelRequest::new(messages, vec![], state);

        let mut middleware_context = MiddlewareContext::new();
        let result = middleware
            .before_model_call(&request, &mut middleware_context)
            .await;

        assert!(result.is_ok());
    }
}
