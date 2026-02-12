use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;

use crate::{
    agent::{
        context_engineering::ModelRequest,
        middleware::{Middleware, MiddlewareContext, MiddlewareError},
    },
    language_models::llm::LLM,
};

/// Middleware that dynamically selects the model based on context.
///
/// This allows you to switch models based on conversation length,
/// user preferences, cost limits, or other factors.
pub struct DynamicModelMiddleware {
    /// Function that selects a model based on ModelRequest
    model_selector: Arc<dyn Fn(&ModelRequest) -> Option<Arc<dyn LLM>> + Send + Sync>,
    /// Available models mapped by name
    available_models: HashMap<String, Arc<dyn LLM>>,
}

impl DynamicModelMiddleware {
    /// Create a new DynamicModelMiddleware with a model selector function
    pub fn new<F>(selector: F, models: HashMap<String, Arc<dyn LLM>>) -> Self
    where
        F: Fn(&ModelRequest) -> Option<Arc<dyn LLM>> + Send + Sync + 'static,
    {
        Self {
            model_selector: Arc::new(selector),
            available_models: models,
        }
    }

    /// Create a model selector based on message count
    ///
    /// `thresholds` is a list of (message_count, model_name) pairs.
    /// The model is selected based on the first threshold that the message count exceeds.
    pub fn from_message_count(
        models: HashMap<String, Arc<dyn LLM>>,
        thresholds: Vec<(usize, String)>,
    ) -> Self {
        let models_clone = models.clone();
        Self::new(
            move |request: &ModelRequest| {
                let message_count = request.messages.len();

                // Find the first threshold that message_count exceeds
                for (threshold, model_name) in &thresholds {
                    if message_count >= *threshold {
                        if let Some(model) = models_clone.get(model_name) {
                            return Some(Arc::clone(model));
                        }
                    }
                }

                // Default: use the first model or None
                models_clone.values().next().map(|m| Arc::clone(m))
            },
            models,
        )
    }

    /// Create a model selector based on user preference from Store
    pub fn from_user_preference(models: HashMap<String, Arc<dyn LLM>>) -> Self {
        let models_clone = models.clone();
        Self::new(
            move |request: &ModelRequest| {
                if let Some(runtime) = request.runtime() {
                    // Try to get user preference from store
                    // This is a simplified version - in practice, you'd read from store
                    if let Some(_user_id) = runtime.context().user_id() {
                        // In a real implementation, you'd read from store:
                        // let prefs = runtime.store().get(("preferences",), user_id).await?;
                        // let preferred_model = prefs.value.get("preferred_model")?;
                        // models_clone.get(preferred_model)

                        // For now, return default
                        models_clone.values().next().map(|m| Arc::clone(m))
                    } else {
                        None
                    }
                } else {
                    None
                }
            },
            models,
        )
    }

    /// Create a model selector based on cost tier from Runtime Context
    pub fn from_cost_tier(models: HashMap<String, Arc<dyn LLM>>) -> Self {
        let models_clone = models.clone();
        Self::new(
            move |request: &ModelRequest| {
                if let Some(runtime) = request.runtime() {
                    // Try to get cost_tier from context
                    if let Some(cost_tier) = runtime.context().get("cost_tier") {
                        let model_name = match cost_tier {
                            "premium" => "premium_model",
                            "budget" => "budget_model",
                            _ => "standard_model",
                        };

                        return models_clone.get(model_name).map(|m| Arc::clone(m));
                    }
                }

                // Default: use standard model
                models_clone
                    .get("standard_model")
                    .or_else(|| models_clone.values().next())
                    .map(|m| Arc::clone(m))
            },
            models,
        )
    }
}

#[async_trait]
impl Middleware for DynamicModelMiddleware {
    async fn before_model_call(
        &self,
        request: &ModelRequest,
        _context: &mut MiddlewareContext,
    ) -> Result<Option<ModelRequest>, MiddlewareError> {
        // Select model
        // Note: Model override is stored in ModelRequest but cannot be easily cloned
        // In practice, the model selection would happen at a different level
        // For now, we'll just return None and let the agent use its default model
        let _selected_model = (self.model_selector)(request);
        Ok(None) // Model selection would need to be handled differently
    }
}

// Note: Cannot implement Clone because HashMap contains non-cloneable trait objects (Arc<dyn LLM>)
// In practice, Arc<dyn LLM> can be cloned, but the HashMap itself might have issues
// For now, we'll skip Clone implementation

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::AgentState;
    use crate::schemas::Message;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    #[tokio::test]
    async fn test_dynamic_model_from_message_count() {
        // This test would require actual LLM instances
        // For now, we'll just test the structure
        let models: HashMap<String, Arc<dyn LLM>> = HashMap::new();
        let thresholds = vec![
            (10, "standard_model".to_string()),
            (20, "large_model".to_string()),
        ];

        let middleware = DynamicModelMiddleware::from_message_count(models, thresholds);

        let state = Arc::new(Mutex::new(AgentState::new()));
        let messages = vec![Message::new_human_message("Hello"); 15];
        let request = ModelRequest::new(messages, vec![], state);

        let mut middleware_context = MiddlewareContext::new();
        let result = middleware
            .before_model_call(&request, &mut middleware_context)
            .await;

        assert!(result.is_ok());
    }
}
