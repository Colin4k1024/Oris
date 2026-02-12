//! Sequential Thinking tool: chain-of-thought reasoning via an LLM.
//!
//! The agent can call this tool with a thought or question; the tool invokes the LLM
//! with a step-by-step reasoning prompt and returns the reasoning text.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::language_models::llm::LLM;
use crate::schemas::Message;

use crate::tools::Tool;

/// Tool that performs step-by-step (chain-of-thought) reasoning using an LLM.
///
/// Construct with an `Arc<dyn LLM>` (e.g. the same LLM as the agent, or a dedicated
/// reasoning model). When the agent calls the tool with a thought or question, the LLM
/// is invoked with a CoT-style prompt and the response is returned as the observation.
pub struct SequentialThinking {
    llm: Arc<dyn LLM>,
    system_prompt: String,
}

impl SequentialThinking {
    /// Default system prompt for chain-of-thought reasoning.
    pub const DEFAULT_SYSTEM_PROMPT: &'static str =
        "Think step by step. Be concise. Reason through the given thought or question and output your reasoning.";

    /// Create a new SequentialThinking tool with the given LLM.
    pub fn new(llm: Arc<dyn LLM>) -> Self {
        Self {
            llm,
            system_prompt: Self::DEFAULT_SYSTEM_PROMPT.to_string(),
        }
    }

    /// Customize the system prompt used for reasoning.
    pub fn with_system_prompt<S: Into<String>>(mut self, prompt: S) -> Self {
        self.system_prompt = prompt.into();
        self
    }
}

#[async_trait]
impl Tool for SequentialThinking {
    fn name(&self) -> String {
        "Sequential_Thinking".to_string()
    }

    fn description(&self) -> String {
        "Performs step-by-step reasoning on the given thought or question. Use this when you need to reason through a problem before acting. The input should be the current thought or question to reason about. The tool returns the reasoning text from the model.".to_string()
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "input": {
                    "type": "string",
                    "description": "The current thought or question to reason about step by step."
                }
            },
            "required": ["input"]
        })
    }

    async fn parse_input(&self, input: &str) -> Value {
        match serde_json::from_str::<Value>(input) {
            Ok(v) => {
                if let Some(s) = v.get("input").and_then(|x| x.as_str()) {
                    Value::String(s.to_string())
                } else {
                    Value::String(input.to_string())
                }
            }
            Err(_) => Value::String(input.to_string()),
        }
    }

    async fn run(&self, input: Value) -> Result<String, crate::error::ToolError> {
        let thought = input
            .as_str()
            .ok_or_else(|| {
                crate::error::ToolError::InvalidInputError("input must be a string".into())
            })?
            .trim();
        if thought.is_empty() {
            return Err(crate::error::ToolError::InvalidInputError(
                "input must not be empty".into(),
            ));
        }

        let system = Message::new_system_message(&self.system_prompt);
        let user = Message::new_human_message(thought);
        let messages = [system, user];

        let result = self
            .llm
            .generate(&messages)
            .await
            .map_err(|e| crate::error::ToolError::ExecutionError(e.to_string()))?;

        Ok(result.generation)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::language_models::{GenerateResult, LLMError};
    use crate::schemas::{Message, StreamData};
    use async_trait::async_trait;
    use futures::stream;
    use std::pin::Pin;

    #[derive(Clone)]
    struct MockLLM {
        response: String,
    }

    #[async_trait]
    impl crate::language_models::llm::LLM for MockLLM {
        async fn generate(&self, _messages: &[Message]) -> Result<GenerateResult, LLMError> {
            Ok(GenerateResult {
                generation: self.response.clone(),
                ..Default::default()
            })
        }

        async fn invoke(&self, _prompt: &str) -> Result<String, LLMError> {
            Ok(self.response.clone())
        }

        async fn stream(
            &self,
            _messages: &[Message],
        ) -> Result<
            Pin<Box<dyn futures::Stream<Item = Result<StreamData, LLMError>> + Send>>,
            LLMError,
        > {
            let response = self.response.clone();
            Ok(Box::pin(stream::once(async move {
                Ok(StreamData::new(serde_json::Value::Null, None, response))
            })))
        }
    }

    #[tokio::test]
    async fn test_sequential_thinking_name_and_description() {
        let llm = Arc::new(MockLLM {
            response: "reasoning".to_string(),
        });
        let tool = SequentialThinking::new(llm);
        assert_eq!(tool.name(), "Sequential_Thinking");
        assert!(tool.description().contains("step-by-step"));
    }

    #[tokio::test]
    async fn test_sequential_thinking_parse_input() {
        let llm = Arc::new(MockLLM {
            response: "ok".to_string(),
        });
        let tool = SequentialThinking::new(llm);
        let v = tool.parse_input(r#"{"input": "what is 2+2?"}"#).await;
        assert_eq!(v, serde_json::Value::String("what is 2+2?".to_string()));
    }

    #[tokio::test]
    async fn test_sequential_thinking_run_returns_llm_output() {
        let llm = Arc::new(MockLLM {
            response: "Step 1: Two. Step 2: Four.".to_string(),
        });
        let tool = SequentialThinking::new(llm);
        let result = tool
            .run(serde_json::Value::String("what is 2+2?".to_string()))
            .await
            .unwrap();
        assert_eq!(result, "Step 1: Two. Step 2: Four.");
    }

    #[tokio::test]
    async fn test_sequential_thinking_run_empty_input_errors() {
        let llm = Arc::new(MockLLM {
            response: "ok".to_string(),
        });
        let tool = SequentialThinking::new(llm);
        let result = tool.run(serde_json::Value::String("   ".to_string())).await;
        assert!(result.is_err());
    }
}
