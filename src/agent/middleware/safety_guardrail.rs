use async_trait::async_trait;
use std::sync::Arc;

use super::{Middleware, MiddlewareContext, MiddlewareError};
use crate::language_models::llm::LLM;
use crate::prompt::PromptArgs;
use crate::schemas::agent::{AgentEvent, AgentFinish};
use crate::schemas::messages::Message;

/// Model-based guardrail: Use an LLM to evaluate response safety.
///
/// This middleware uses a separate LLM to evaluate whether the agent's
/// response is safe and appropriate before returning it to the user.
///
/// # Example
/// ```rust,ignore
/// use oris::agent::middleware::SafetyGuardrailMiddleware;
/// use oris::language_models::init_chat_model;
///
/// let safety_model = init_chat_model("gpt-4o-mini", None, None, None, None, None, None, None)?;
/// let middleware = SafetyGuardrailMiddleware::new(safety_model)
///     .with_safety_prompt("Evaluate if this response is safe. Respond with only 'SAFE' or 'UNSAFE'.");
/// ```
pub struct SafetyGuardrailMiddleware {
    safety_model: Arc<dyn LLM>,
    safety_prompt: String,
    unsafe_response_message: String,
}

impl SafetyGuardrailMiddleware {
    pub fn new(safety_model: Arc<dyn LLM>) -> Self {
        Self {
            safety_model,
            safety_prompt: "Evaluate if this response is safe and appropriate. Respond with only 'SAFE' or 'UNSAFE'.".to_string(),
            unsafe_response_message: "I cannot provide that response. Please rephrase your request.".to_string(),
        }
    }

    /// Set the prompt used for safety evaluation.
    pub fn with_safety_prompt(mut self, prompt: String) -> Self {
        self.safety_prompt = prompt;
        self
    }

    /// Set the message to return when response is deemed unsafe.
    pub fn with_unsafe_response_message(mut self, message: String) -> Self {
        self.unsafe_response_message = message;
        self
    }

    /// Evaluate if a response is safe using the safety model.
    async fn evaluate_safety(&self, response: &str) -> Result<bool, MiddlewareError> {
        let evaluation_prompt = format!("{}\n\nResponse: {}", self.safety_prompt, response);

        let messages = vec![Message::new_human_message(&evaluation_prompt)];
        let result =
            self.safety_model.generate(&messages).await.map_err(|e| {
                MiddlewareError::ExecutionError(format!("Safety model error: {}", e))
            })?;

        let evaluation = result.generation.trim().to_uppercase();

        // Check if response indicates safety
        let is_safe = evaluation.contains("SAFE") && !evaluation.contains("UNSAFE");

        Ok(is_safe)
    }
}

#[async_trait]
impl Middleware for SafetyGuardrailMiddleware {
    async fn after_agent_plan(
        &self,
        _input: &PromptArgs,
        event: &AgentEvent,
        context: &mut MiddlewareContext,
    ) -> Result<Option<AgentEvent>, MiddlewareError> {
        // Only evaluate finish events (final responses)
        if let AgentEvent::Finish(finish) = event {
            let is_safe = self.evaluate_safety(&finish.output).await?;

            context.set_custom_data("safety_evaluated".to_string(), serde_json::json!(true));
            context.set_custom_data("is_safe".to_string(), serde_json::json!(is_safe));

            if !is_safe {
                log::warn!("Safety guardrail blocked unsafe response");

                // Replace with safe message
                let mut modified_finish = finish.clone();
                modified_finish.output = self.unsafe_response_message.clone();
                return Ok(Some(AgentEvent::Finish(modified_finish)));
            }
        }

        Ok(None)
    }

    async fn before_finish(
        &self,
        finish: &AgentFinish,
        context: &mut MiddlewareContext,
    ) -> Result<Option<AgentFinish>, MiddlewareError> {
        // Double-check safety before final return
        let is_safe = self.evaluate_safety(&finish.output).await?;

        if !is_safe {
            log::warn!("Safety guardrail blocked unsafe response in before_finish");

            context.set_custom_data("safety_blocked".to_string(), serde_json::json!(true));

            let mut modified_finish = finish.clone();
            modified_finish.output = self.unsafe_response_message.clone();
            return Ok(Some(modified_finish));
        }

        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::language_models::llm::LLM;
    use crate::language_models::GenerateResult;
    use crate::schemas::messages::Message;
    use crate::schemas::StreamData;
    use async_trait::async_trait;
    use futures::Stream;
    use std::sync::Arc;

    // Mock LLM for testing
    #[derive(Clone)]
    struct MockSafetyModel {
        response: String,
    }

    #[async_trait]
    impl LLM for MockSafetyModel {
        async fn generate(
            &self,
            _messages: &[Message],
        ) -> Result<GenerateResult, crate::language_models::LLMError> {
            Ok(GenerateResult {
                generation: self.response.clone(),
                ..Default::default()
            })
        }

        async fn invoke(&self, _prompt: &str) -> Result<String, crate::language_models::LLMError> {
            Ok(self.response.clone())
        }

        async fn stream(
            &self,
            _messages: &[Message],
        ) -> Result<
            std::pin::Pin<
                Box<dyn Stream<Item = Result<StreamData, crate::language_models::LLMError>> + Send>,
            >,
            crate::language_models::LLMError,
        > {
            use futures::stream;
            use std::pin::Pin;
            let response = self.response.clone();
            Ok(Box::pin(stream::once(async move {
                Ok(StreamData::new(serde_json::Value::Null, None, response))
            })))
        }

        fn add_options(&mut self, _options: crate::language_models::options::CallOptions) {}
    }

    #[tokio::test]
    async fn test_safety_evaluation_safe() {
        let mock_model = Arc::new(MockSafetyModel {
            response: "SAFE".to_string(),
        });
        let middleware = SafetyGuardrailMiddleware::new(mock_model);

        let is_safe = middleware
            .evaluate_safety("This is a safe response")
            .await
            .unwrap();
        assert!(is_safe);
    }

    #[tokio::test]
    async fn test_safety_evaluation_unsafe() {
        let mock_model = Arc::new(MockSafetyModel {
            response: "UNSAFE".to_string(),
        });
        let middleware = SafetyGuardrailMiddleware::new(mock_model);

        let is_safe = middleware
            .evaluate_safety("This is an unsafe response")
            .await
            .unwrap();
        assert!(!is_safe);
    }
}
