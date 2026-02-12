use std::sync::Arc;

use async_trait::async_trait;
use serde_json::json;

use super::{Middleware, MiddlewareContext, MiddlewareError};
use crate::language_models::llm::LLM;
use crate::prompt::PromptArgs;
use crate::schemas::agent::AgentAction;
use crate::schemas::Message;

/// Summarization middleware for managing long conversation history.
///
/// When a summarizer LLM is configured and thresholds are exceeded, older
/// messages are summarized into a single system message and recent messages
/// are preserved. Aligns with [harness – Conversation history summarization](https://docs.langchain.com/oss/python/deepagents/harness#conversation-history-summarization).
///
/// # Example
/// ```rust,ignore
/// use oris::agent::middleware::SummarizationMiddleware;
///
/// let middleware = SummarizationMiddleware::new()
///     .with_token_threshold(4000)
///     .with_message_threshold(50)
///     .with_preserve_recent(10)
///     .with_summarizer(some_llm);
/// ```
pub struct SummarizationMiddleware {
    token_threshold: Option<usize>,
    message_threshold: Option<usize>,
    preserve_recent: usize,
    summarization_prompt: String,
    summarizer: Option<Arc<dyn LLM>>,
}

impl SummarizationMiddleware {
    pub fn new() -> Self {
        Self {
            token_threshold: None,
            message_threshold: Some(50),
            preserve_recent: 10,
            summarization_prompt:
                "Summarize the following conversation history, preserving key facts, decisions, and context:"
                    .to_string(),
            summarizer: None,
        }
    }

    pub fn with_token_threshold(mut self, threshold: usize) -> Self {
        self.token_threshold = Some(threshold);
        self
    }

    pub fn with_message_threshold(mut self, threshold: usize) -> Self {
        self.message_threshold = Some(threshold);
        self
    }

    pub fn with_preserve_recent(mut self, count: usize) -> Self {
        self.preserve_recent = count;
        self
    }

    pub fn with_summarization_prompt(mut self, prompt: String) -> Self {
        self.summarization_prompt = prompt;
        self
    }

    /// Set the LLM used to summarize; if None, summarization is not performed (only thresholds are logged).
    pub fn with_summarizer(mut self, summarizer: Arc<dyn LLM>) -> Self {
        self.summarizer = Some(summarizer);
        self
    }

    fn should_summarize(&self, message_count: usize, estimated_tokens: usize) -> bool {
        if let Some(threshold) = self.message_threshold {
            if message_count > threshold {
                return true;
            }
        }
        if let Some(threshold) = self.token_threshold {
            if estimated_tokens > threshold {
                return true;
            }
        }
        false
    }

    fn estimate_tokens(messages: &[Message]) -> usize {
        let total_chars: usize = messages.iter().map(|m| m.content.chars().count()).sum();
        total_chars / 4
    }

    async fn summarize_history(&self, history_text: &str) -> Result<String, MiddlewareError> {
        let llm = match &self.summarizer {
            Some(l) => l,
            None => return Ok("[Summarized conversation history]".to_string()),
        };
        let messages = [
            Message::new_system_message(&self.summarization_prompt),
            Message::new_human_message(history_text),
        ];
        let result = llm
            .generate(&messages)
            .await
            .map_err(|e| MiddlewareError::ExecutionError(e.to_string()))?;
        Ok(result.generation.trim().to_string())
    }
}

impl Default for SummarizationMiddleware {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Middleware for SummarizationMiddleware {
    async fn before_agent_plan(
        &self,
        input: &PromptArgs,
        _steps: &[(AgentAction, String)],
        context: &mut MiddlewareContext,
    ) -> Result<Option<PromptArgs>, MiddlewareError> {
        let chat_history_value = match input.get("chat_history") {
            Some(v) => v,
            None => return Ok(None),
        };
        let mut messages: Vec<Message> = match serde_json::from_value(chat_history_value.clone()) {
            Ok(m) => m,
            Err(_) => return Ok(None),
        };

        let message_count = messages.len();
        let estimated_tokens = Self::estimate_tokens(&messages);
        if !self.should_summarize(message_count, estimated_tokens) {
            return Ok(None);
        }

        if self.summarizer.is_none() {
            context.set_custom_data("should_summarize".to_string(), json!(true));
            log::info!(
                "Summarization triggered: {} messages, ~{} tokens (no summarizer configured)",
                message_count,
                estimated_tokens
            );
            return Ok(None);
        }

        if messages.len() <= self.preserve_recent {
            return Ok(None);
        }

        let split_at = messages.len().saturating_sub(self.preserve_recent);
        let to_summarize = messages.drain(..split_at).collect::<Vec<_>>();
        let recent = messages;
        let history_text: String = to_summarize
            .iter()
            .map(|m| format!("{:?}: {}", m.message_type, m.content))
            .collect::<Vec<_>>()
            .join("\n");

        let summary = self.summarize_history(&history_text).await?;
        let summary_message =
            Message::new_system_message(format!("[Previous conversation summary]\n{}", summary));
        let new_history: Vec<Message> = std::iter::once(summary_message).chain(recent).collect();

        log::info!(
            "Summarized {} messages to 1 + {} recent",
            to_summarize.len(),
            new_history.len() - 1
        );

        let mut new_input = input.clone();
        new_input.insert("chat_history".to_string(), json!(new_history));
        Ok(Some(new_input))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_summarization_middleware_creation() {
        let middleware = SummarizationMiddleware::new();
        assert_eq!(middleware.preserve_recent, 10);
    }

    #[test]
    fn test_should_summarize() {
        let middleware = SummarizationMiddleware::new().with_message_threshold(50);
        assert!(!middleware.should_summarize(40, 0));
        assert!(middleware.should_summarize(60, 0));
    }
}
