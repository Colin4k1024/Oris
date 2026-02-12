use async_trait::async_trait;

use super::{Middleware, MiddlewareContext, MiddlewareError};
use crate::prompt::PromptArgs;
use crate::schemas::agent::AgentAction;

/// Deterministic guardrail: Block requests containing banned keywords.
///
/// This middleware checks for banned keywords in user input and blocks
/// execution before any processing begins. Useful for preventing
/// inappropriate or harmful content from being processed.
///
/// # Example
/// ```rust,ignore
/// use oris::agent::middleware::ContentFilterMiddleware;
///
/// let middleware = ContentFilterMiddleware::new()
///     .with_banned_keywords(vec!["hack", "exploit", "malware"]);
/// ```
pub struct ContentFilterMiddleware {
    banned_keywords: Vec<String>,
    case_sensitive: bool,
    block_message: String,
}

impl ContentFilterMiddleware {
    pub fn new() -> Self {
        Self {
            banned_keywords: Vec::new(),
            case_sensitive: false,
            block_message: "I cannot process requests containing inappropriate content. Please rephrase your request.".to_string(),
        }
    }

    /// Add banned keywords to filter.
    pub fn with_banned_keywords(mut self, keywords: Vec<String>) -> Self {
        self.banned_keywords = keywords;
        self
    }

    /// Add a single banned keyword.
    pub fn with_banned_keyword(mut self, keyword: String) -> Self {
        self.banned_keywords.push(keyword);
        self
    }

    /// Set whether keyword matching should be case-sensitive.
    pub fn with_case_sensitive(mut self, case_sensitive: bool) -> Self {
        self.case_sensitive = case_sensitive;
        self
    }

    /// Set the message to return when content is blocked.
    pub fn with_block_message(mut self, message: String) -> Self {
        self.block_message = message;
        self
    }

    /// Check if text contains any banned keywords.
    fn contains_banned_keywords(&self, text: &str) -> Option<String> {
        let search_text = if self.case_sensitive {
            text.to_string()
        } else {
            text.to_lowercase()
        };

        for keyword in &self.banned_keywords {
            let search_keyword = if self.case_sensitive {
                keyword.clone()
            } else {
                keyword.to_lowercase()
            };

            if search_text.contains(&search_keyword) {
                return Some(keyword.clone());
            }
        }

        None
    }

    /// Extract text content from prompt args.
    fn extract_text_from_input(&self, input: &PromptArgs) -> String {
        let mut texts = Vec::new();

        // Check "input" field
        if let Some(input_val) = input.get("input") {
            if let Some(s) = input_val.as_str() {
                texts.push(s.to_string());
            }
        }

        // Check "messages" field
        if let Some(messages_val) = input.get("messages") {
            if let Some(messages_array) = messages_val.as_array() {
                for msg in messages_array {
                    if let Some(content) = msg.get("content").and_then(|v| v.as_str()) {
                        texts.push(content.to_string());
                    }
                }
            }
        }

        texts.join(" ")
    }
}

impl Default for ContentFilterMiddleware {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Middleware for ContentFilterMiddleware {
    async fn before_agent_plan(
        &self,
        input: &PromptArgs,
        _steps: &[(AgentAction, String)],
        context: &mut MiddlewareContext,
    ) -> Result<Option<PromptArgs>, MiddlewareError> {
        let text = self.extract_text_from_input(input);

        if let Some(banned_keyword) = self.contains_banned_keywords(&text) {
            // Log the blocked attempt
            log::warn!(
                "Content filter blocked request containing banned keyword: '{}'",
                banned_keyword
            );

            context.set_custom_data("content_filtered".to_string(), serde_json::json!(true));
            context.set_custom_data(
                "banned_keyword".to_string(),
                serde_json::json!(banned_keyword),
            );

            // Return a modified input that will result in the block message
            // This is a workaround - ideally we'd abort execution, but we need
            // to return a valid response. We'll modify the input to generate
            // the block message.
            let mut modified_input = input.clone();
            modified_input.insert(
                "input".to_string(),
                serde_json::json!(self.block_message.clone()),
            );

            // Note: In a real implementation, you might want to raise an error
            // or use a different mechanism to block execution. For now, we'll
            // modify the input to return the block message.
            return Err(MiddlewareError::Aborted(format!(
                "Content filter blocked request: {}",
                self.block_message
            )));
        }

        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_content_filter_creation() {
        let middleware = ContentFilterMiddleware::new();
        assert!(middleware.banned_keywords.is_empty());
        assert!(!middleware.case_sensitive);
    }

    #[test]
    fn test_banned_keyword_detection() {
        let middleware = ContentFilterMiddleware::new()
            .with_banned_keywords(vec!["hack".to_string(), "exploit".to_string()]);

        assert!(middleware
            .contains_banned_keywords("How to hack a system")
            .is_some());
        assert!(middleware
            .contains_banned_keywords("This is safe content")
            .is_none());
    }

    #[test]
    fn test_case_insensitive_matching() {
        let middleware = ContentFilterMiddleware::new()
            .with_banned_keyword("HACK".to_string())
            .with_case_sensitive(false);

        assert!(middleware.contains_banned_keywords("hack").is_some());
        assert!(middleware.contains_banned_keywords("HACK").is_some());
        assert!(middleware.contains_banned_keywords("Hack").is_some());
    }

    #[test]
    fn test_case_sensitive_matching() {
        let middleware = ContentFilterMiddleware::new()
            .with_banned_keyword("HACK".to_string())
            .with_case_sensitive(true);

        assert!(middleware.contains_banned_keywords("HACK").is_some());
        assert!(middleware.contains_banned_keywords("hack").is_none());
    }
}
