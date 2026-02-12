use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

use super::{Middleware, MiddlewareContext, MiddlewareError};
use crate::prompt::PromptArgs;
use crate::schemas::agent::AgentAction;

/// Rate limiting middleware using token bucket algorithm.
///
/// Limits the rate of agent calls and tool calls to prevent overwhelming
/// external services or exceeding API rate limits.
///
/// # Example
/// ```rust,ignore
/// use oris::agent::middleware::RateLimitMiddleware;
/// use std::time::Duration;
///
/// let middleware = RateLimitMiddleware::new()
///     .with_requests_per_second(10)
///     .with_per_tool_limits(vec![("api_call".to_string(), 5)]);
/// ```
pub struct RateLimitMiddleware {
    requests_per_second: Option<u32>,
    requests_per_minute: Option<u32>,
    per_tool_limits: HashMap<String, u32>,
    last_request: Arc<Mutex<Option<Instant>>>,
    request_times: Arc<Mutex<Vec<Instant>>>,
    tool_request_times: Arc<Mutex<HashMap<String, Vec<Instant>>>>,
}

impl RateLimitMiddleware {
    pub fn new() -> Self {
        Self {
            requests_per_second: None,
            requests_per_minute: None,
            per_tool_limits: HashMap::new(),
            last_request: Arc::new(Mutex::new(None)),
            request_times: Arc::new(Mutex::new(Vec::new())),
            tool_request_times: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn with_requests_per_second(mut self, rps: u32) -> Self {
        self.requests_per_second = Some(rps);
        self
    }

    pub fn with_requests_per_minute(mut self, rpm: u32) -> Self {
        self.requests_per_minute = Some(rpm);
        self
    }

    pub fn with_per_tool_limit(mut self, tool_name: String, limit: u32) -> Self {
        self.per_tool_limits.insert(tool_name, limit);
        self
    }

    async fn check_rate_limit(&self) -> Result<(), MiddlewareError> {
        let now = Instant::now();
        let mut request_times = self.request_times.lock().await;

        // Check per-second limit
        if let Some(rps) = self.requests_per_second {
            request_times.retain(|&time| now.duration_since(time) < Duration::from_secs(1));
            if request_times.len() >= rps as usize {
                return Err(MiddlewareError::ValidationError(format!(
                    "Rate limit exceeded: {} requests per second",
                    rps
                )));
            }
        }

        // Check per-minute limit
        if let Some(rpm) = self.requests_per_minute {
            request_times.retain(|&time| now.duration_since(time) < Duration::from_secs(60));
            if request_times.len() >= rpm as usize {
                return Err(MiddlewareError::ValidationError(format!(
                    "Rate limit exceeded: {} requests per minute",
                    rpm
                )));
            }
        }

        request_times.push(now);
        Ok(())
    }

    async fn check_tool_rate_limit(&self, tool_name: &str) -> Result<(), MiddlewareError> {
        if let Some(&limit) = self.per_tool_limits.get(tool_name) {
            let now = Instant::now();
            let mut tool_times = self.tool_request_times.lock().await;
            let times = tool_times
                .entry(tool_name.to_string())
                .or_insert_with(Vec::new);

            // Check requests in the last minute
            times.retain(|&time| now.duration_since(time) < Duration::from_secs(60));
            if times.len() >= limit as usize {
                return Err(MiddlewareError::ValidationError(format!(
                    "Rate limit exceeded for tool {}: {} requests per minute",
                    tool_name, limit
                )));
            }

            times.push(now);
        }
        Ok(())
    }
}

impl Default for RateLimitMiddleware {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Middleware for RateLimitMiddleware {
    async fn before_agent_plan(
        &self,
        _input: &PromptArgs,
        _steps: &[(AgentAction, String)],
        _context: &mut MiddlewareContext,
    ) -> Result<Option<PromptArgs>, MiddlewareError> {
        self.check_rate_limit().await?;
        Ok(None)
    }

    async fn before_tool_call(
        &self,
        action: &AgentAction,
        _context: &mut MiddlewareContext,
    ) -> Result<Option<AgentAction>, MiddlewareError> {
        self.check_tool_rate_limit(&action.tool).await?;
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rate_limit_middleware_creation() {
        let middleware = RateLimitMiddleware::new();
        assert!(middleware.requests_per_second.is_none());
    }

    #[tokio::test]
    async fn test_rate_limit_check() {
        let middleware = RateLimitMiddleware::new().with_requests_per_second(2);

        // First two should succeed
        assert!(middleware.check_rate_limit().await.is_ok());
        tokio::time::sleep(Duration::from_millis(10)).await;
        assert!(middleware.check_rate_limit().await.is_ok());

        // Third should fail (within same second)
        let result = middleware.check_rate_limit().await;
        assert!(result.is_err());
    }
}
