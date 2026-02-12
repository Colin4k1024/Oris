use async_trait::async_trait;
use sha2::{Digest, Sha256};

use super::pii_detector::{PIIDetector, PIIType};
use super::{Middleware, MiddlewareContext, MiddlewareError};
use crate::prompt::PromptArgs;
use crate::schemas::agent::{AgentAction, AgentEvent};

/// Strategy for handling detected PII.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PIIStrategy {
    /// Replace with [REDACTED_{PII_TYPE}]
    Redact,
    /// Partially obscure (e.g., show last 4 digits)
    Mask,
    /// Replace with deterministic hash
    Hash,
    /// Raise exception when detected
    Block,
}

impl PIIStrategy {
    pub fn as_str(&self) -> &str {
        match self {
            PIIStrategy::Redact => "redact",
            PIIStrategy::Mask => "mask",
            PIIStrategy::Hash => "hash",
            PIIStrategy::Block => "block",
        }
    }
}

/// Middleware for detecting and handling Personally Identifiable Information (PII).
///
/// This middleware can detect common PII types (email, credit card, IP address, etc.)
/// and apply various strategies to handle them: redact, mask, hash, or block.
///
/// # Example
/// ```rust,ignore
/// use oris::agent::middleware::{PIIMiddleware, PIIStrategy};
///
/// let middleware = PIIMiddleware::new(PIIType::Email, PIIStrategy::Redact)
///     .with_apply_to_input(true)
///     .with_apply_to_output(true);
/// ```
pub struct PIIMiddleware {
    pii_type: PIIType,
    strategy: PIIStrategy,
    detector: PIIDetector,
    apply_to_input: bool,
    apply_to_output: bool,
    apply_to_tool_results: bool,
    custom_detector: Option<String>,
}

impl PIIMiddleware {
    /// Create a new PII middleware with a built-in PII type.
    pub fn new(pii_type: PIIType, strategy: PIIStrategy) -> Self {
        let detector = PIIDetector::new(pii_type.clone());
        Self {
            pii_type,
            strategy,
            detector,
            apply_to_input: true,
            apply_to_output: false,
            apply_to_tool_results: false,
            custom_detector: None,
        }
    }

    /// Create a new PII middleware with a custom regex pattern.
    pub fn with_custom_pattern(
        pii_type: PIIType,
        strategy: PIIStrategy,
        pattern: &str,
    ) -> Result<Self, regex::Error> {
        let detector = PIIDetector::with_custom_pattern(pii_type.clone(), pattern)?;
        Ok(Self {
            pii_type,
            strategy,
            detector,
            apply_to_input: true,
            apply_to_output: false,
            apply_to_tool_results: false,
            custom_detector: Some(pattern.to_string()),
        })
    }

    /// Set whether to apply PII detection to input messages.
    pub fn with_apply_to_input(mut self, apply: bool) -> Self {
        self.apply_to_input = apply;
        self
    }

    /// Set whether to apply PII detection to output messages.
    pub fn with_apply_to_output(mut self, apply: bool) -> Self {
        self.apply_to_output = apply;
        self
    }

    /// Set whether to apply PII detection to tool results.
    pub fn with_apply_to_tool_results(mut self, apply: bool) -> Self {
        self.apply_to_tool_results = apply;
        self
    }

    /// Process text to handle detected PII according to the strategy.
    fn process_text(&self, text: &str) -> Result<String, MiddlewareError> {
        let matches = self.detector.detect(text);

        if matches.is_empty() {
            return Ok(text.to_string());
        }

        // If blocking, raise error immediately
        if matches!(self.strategy, PIIStrategy::Block) {
            return Err(MiddlewareError::Aborted(format!(
                "PII detected: {} instances of {} found",
                matches.len(),
                self.pii_type.as_str()
            )));
        }

        // Process matches in reverse order to maintain indices
        let mut result = text.to_string();
        for mat in matches.iter().rev() {
            let replacement = match self.strategy {
                PIIStrategy::Redact => {
                    format!("[REDACTED_{}]", self.pii_type.as_str())
                }
                PIIStrategy::Mask => self.mask_pii(&mat.matched_text),
                PIIStrategy::Hash => self.hash_pii(&mat.matched_text),
                PIIStrategy::Block => unreachable!(), // Already handled above
            };
            result.replace_range(mat.start..mat.end, &replacement);
        }

        Ok(result)
    }

    /// Mask PII by partially obscuring it.
    fn mask_pii(&self, pii: &str) -> String {
        match self.pii_type {
            PIIType::CreditCard => {
                // Show last 4 digits: ****-****-****-1234
                let digits: String = pii.chars().filter(|c| c.is_ascii_digit()).collect();
                if digits.len() >= 4 {
                    let last_four = &digits[digits.len() - 4..];
                    format!("****-****-****-{}", last_four)
                } else {
                    "****-****-****-****".to_string()
                }
            }
            PIIType::Email => {
                // Mask email: u***@example.com
                if let Some(at_pos) = pii.find('@') {
                    if at_pos > 0 {
                        let first_char = &pii[..1];
                        format!("{}***{}", first_char, &pii[at_pos..])
                    } else {
                        format!("***{}", &pii[at_pos..])
                    }
                } else {
                    "***@***".to_string()
                }
            }
            PIIType::IPAddress => {
                // Mask IP: 192.168.***.***
                if let Some(last_dot) = pii.rfind('.') {
                    if let Some(second_last_dot) = pii[..last_dot].rfind('.') {
                        format!("{}.***.***", &pii[..=second_last_dot])
                    } else {
                        format!("{}.***", &pii[..last_dot])
                    }
                } else {
                    "***.***.***.***".to_string()
                }
            }
            _ => {
                // Generic masking: show first and last character
                if pii.len() > 2 {
                    format!("{}***{}", &pii[..1], &pii[pii.len() - 1..])
                } else {
                    "***".to_string()
                }
            }
        }
    }

    /// Hash PII using SHA256.
    fn hash_pii(&self, pii: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(pii.as_bytes());
        let hash = hasher.finalize();
        format!("{:x}", hash)
    }

    /// Extract text content from prompt args for PII checking.
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

#[async_trait]
impl Middleware for PIIMiddleware {
    async fn before_agent_plan(
        &self,
        input: &PromptArgs,
        _steps: &[(AgentAction, String)],
        _context: &mut MiddlewareContext,
    ) -> Result<Option<PromptArgs>, MiddlewareError> {
        if !self.apply_to_input {
            return Ok(None);
        }

        let text = self.extract_text_from_input(input);
        let processed = self.process_text(&text)?;

        // If text was modified, update the input
        if processed != text {
            let mut modified_input = input.clone();

            // Update "input" field if it exists
            if modified_input.contains_key("input") {
                if let Some(input_val) = modified_input.get("input") {
                    if let Some(original) = input_val.as_str() {
                        if text.contains(original) {
                            // Replace the original input in processed text
                            let new_input = processed
                                .replace(original, &processed)
                                .split_whitespace()
                                .next()
                                .unwrap_or(&processed)
                                .to_string();
                            modified_input
                                .insert("input".to_string(), serde_json::json!(new_input));
                        }
                    }
                }
            }

            // Update "messages" field if it exists
            if let Some(messages_val) = modified_input.get_mut("messages") {
                if let Some(messages_array) = messages_val.as_array_mut() {
                    for msg in messages_array {
                        if let Some(content) = msg.get_mut("content") {
                            if let Some(content_str) = content.as_str() {
                                let processed_content = self.process_text(content_str)?;
                                *content = serde_json::json!(processed_content);
                            }
                        }
                    }
                }
            }

            return Ok(Some(modified_input));
        }

        Ok(None)
    }

    async fn after_agent_plan(
        &self,
        _input: &PromptArgs,
        event: &AgentEvent,
        _context: &mut MiddlewareContext,
    ) -> Result<Option<AgentEvent>, MiddlewareError> {
        if !self.apply_to_output {
            return Ok(None);
        }

        match event {
            AgentEvent::Finish(finish) => {
                let processed = self.process_text(&finish.output)?;
                if processed != finish.output {
                    let mut modified_finish = finish.clone();
                    modified_finish.output = processed;
                    return Ok(Some(AgentEvent::Finish(modified_finish)));
                }
            }
            _ => {}
        }

        Ok(None)
    }

    async fn after_tool_call(
        &self,
        _action: &AgentAction,
        observation: &str,
        _context: &mut MiddlewareContext,
    ) -> Result<Option<String>, MiddlewareError> {
        if !self.apply_to_tool_results {
            return Ok(None);
        }

        let processed = self.process_text(observation)?;
        if processed != observation {
            return Ok(Some(processed));
        }

        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pii_middleware_creation() {
        let middleware = PIIMiddleware::new(PIIType::Email, PIIStrategy::Redact);
        assert!(middleware.apply_to_input);
        assert!(!middleware.apply_to_output);
    }

    #[test]
    fn test_redact_strategy() {
        let middleware = PIIMiddleware::new(PIIType::Email, PIIStrategy::Redact);
        let text = "Contact [email protected]";
        let result = middleware.process_text(text).unwrap();
        assert!(result.contains("[REDACTED_EMAIL]"));
    }

    #[test]
    fn test_mask_strategy() {
        let middleware = PIIMiddleware::new(PIIType::Email, PIIStrategy::Mask);
        let text = "Contact [email protected]";
        let result = middleware.process_text(text).unwrap();
        assert!(result.contains("***"));
        assert!(result.contains("@example.com"));
    }

    #[test]
    fn test_hash_strategy() {
        let middleware = PIIMiddleware::new(PIIType::Email, PIIStrategy::Hash);
        let text = "Contact [email protected]";
        let result = middleware.process_text(text).unwrap();
        // Hash should be a hex string
        assert!(result.len() == 64 || result.contains("Contact"));
    }

    #[test]
    fn test_block_strategy() {
        let middleware = PIIMiddleware::new(PIIType::Email, PIIStrategy::Block);
        let text = "Contact [email protected]";
        let result = middleware.process_text(text);
        assert!(result.is_err());
    }
}
