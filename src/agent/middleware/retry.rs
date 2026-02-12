use async_trait::async_trait;
use std::time::Duration;

use super::{Middleware, MiddlewareContext, MiddlewareError};
use crate::schemas::agent::AgentAction;

/// Retry middleware for automatically retrying failed tool calls.
///
/// This middleware intercepts tool call errors and retries them with exponential backoff.
///
/// # Example
/// ```rust,ignore
/// use oris::agent::middleware::RetryMiddleware;
///
/// let middleware = RetryMiddleware::new()
///     .with_max_retries(3)
///     .with_initial_delay(Duration::from_secs(1));
/// ```
pub struct RetryMiddleware {
    max_retries: u32,
    initial_delay: Duration,
    max_delay: Duration,
    retryable_errors: Vec<String>,
}

impl RetryMiddleware {
    pub fn new() -> Self {
        Self {
            max_retries: 3,
            initial_delay: Duration::from_millis(100),
            max_delay: Duration::from_secs(10),
            retryable_errors: Vec::new(),
        }
    }

    pub fn with_max_retries(mut self, max_retries: u32) -> Self {
        self.max_retries = max_retries;
        self
    }

    pub fn with_initial_delay(mut self, delay: Duration) -> Self {
        self.initial_delay = delay;
        self
    }

    pub fn with_max_delay(mut self, delay: Duration) -> Self {
        self.max_delay = delay;
        self
    }

    pub fn with_retryable_errors(mut self, errors: Vec<String>) -> Self {
        self.retryable_errors = errors;
        self
    }

    fn should_retry(&self, error: &str, retry_count: u32) -> bool {
        if retry_count >= self.max_retries {
            return false;
        }

        if self.retryable_errors.is_empty() {
            return true; // Retry all errors by default
        }

        self.retryable_errors.iter().any(|e| error.contains(e))
    }

    async fn calculate_delay(&self, retry_count: u32) -> Duration {
        let delay_ms = self.initial_delay.as_millis() as u64 * 2_u64.pow(retry_count);
        let delay = Duration::from_millis(delay_ms.min(self.max_delay.as_millis() as u64));
        delay
    }
}

impl Default for RetryMiddleware {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Middleware for RetryMiddleware {
    // Retry middleware primarily works by intercepting errors in after_tool_call
    // and storing retry state in context. The actual retry logic would need to be
    // implemented at the executor level, but this provides the configuration.

    async fn after_tool_call(
        &self,
        _action: &AgentAction,
        observation: &str,
        context: &mut MiddlewareContext,
    ) -> Result<Option<String>, MiddlewareError> {
        // Check if observation indicates an error that should be retried
        if observation.contains("error") || observation.contains("Error") {
            let retry_count = context
                .get_custom_data("retry_count")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32;

            if self.should_retry(observation, retry_count) {
                // Store retry information in context
                context.set_custom_data("should_retry".to_string(), serde_json::json!(true));
                context.set_custom_data(
                    "retry_count".to_string(),
                    serde_json::json!(retry_count + 1),
                );
                context.set_custom_data(
                    "retry_delay_ms".to_string(),
                    serde_json::json!(self.calculate_delay(retry_count).await.as_millis()),
                );

                // Note: Actual retry logic would be implemented in the executor
                // This middleware just marks that a retry should happen
            }
        } else {
            // Clear retry state on success
            context.set_custom_data("should_retry".to_string(), serde_json::json!(false));
            context.set_custom_data("retry_count".to_string(), serde_json::json!(0));
        }

        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_retry_middleware_creation() {
        let middleware = RetryMiddleware::new();
        assert_eq!(middleware.max_retries, 3);
    }

    #[tokio::test]
    async fn test_should_retry() {
        let middleware = RetryMiddleware::new().with_max_retries(2);
        assert!(middleware.should_retry("Some error", 0));
        assert!(middleware.should_retry("Some error", 1));
        assert!(!middleware.should_retry("Some error", 2));
    }

    #[tokio::test]
    async fn test_calculate_delay() {
        let middleware = RetryMiddleware::new()
            .with_initial_delay(Duration::from_millis(100))
            .with_max_delay(Duration::from_secs(1));

        let delay0 = middleware.calculate_delay(0).await;
        assert_eq!(delay0, Duration::from_millis(100));

        let delay1 = middleware.calculate_delay(1).await;
        assert_eq!(delay1, Duration::from_millis(200));
    }
}
