//! Evict large tool results to the store to avoid context window saturation.
//!
//! See [harness – Large tool result eviction](https://docs.langchain.com/oss/python/deepagents/harness#large-tool-result-eviction).

use async_trait::async_trait;
use serde_json::json;

use super::{Middleware, MiddlewareContext, MiddlewareError};
use crate::agent::runtime::Runtime;
use crate::schemas::agent::AgentAction;
use crate::schemas::LogTools;

const TOOL_EVICTION_NAMESPACE: &[&str] = &["tool_eviction"];
const DEFAULT_PREVIEW_CHARS: usize = 1500;

/// Middleware that writes large tool results to the runtime store and replaces
/// them with a short preview, so the context window is not saturated.
pub struct ToolResultEvictionMiddleware {
    /// Evict when estimated token count exceeds this (None = disabled).
    token_limit: Option<usize>,
    /// Approximate chars per token for estimation.
    chars_per_token: usize,
    /// Max chars to keep in the observation as preview.
    preview_chars: usize,
}

impl ToolResultEvictionMiddleware {
    pub fn new() -> Self {
        Self {
            token_limit: Some(20_000),
            chars_per_token: 4,
            preview_chars: DEFAULT_PREVIEW_CHARS,
        }
    }

    /// Disable eviction (pass None for token_limit).
    pub fn with_token_limit(mut self, limit: Option<usize>) -> Self {
        self.token_limit = limit;
        self
    }

    pub fn with_preview_chars(mut self, chars: usize) -> Self {
        self.preview_chars = chars;
        self
    }

    fn estimated_tokens(&self, observation: &str) -> usize {
        observation.chars().count() / self.chars_per_token.max(1)
    }

    fn tool_call_id_from_action(action: &AgentAction) -> Option<String> {
        serde_json::from_str::<LogTools>(&action.log)
            .ok()
            .map(|l| l.tool_id)
    }
}

impl Default for ToolResultEvictionMiddleware {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Middleware for ToolResultEvictionMiddleware {
    async fn after_tool_call_with_runtime(
        &self,
        action: &AgentAction,
        observation: &str,
        runtime: Option<&Runtime>,
        _context: &mut MiddlewareContext,
    ) -> Result<Option<String>, MiddlewareError> {
        let limit = match self.token_limit {
            Some(l) => l,
            None => return Ok(None),
        };
        let runtime = match runtime {
            Some(r) => r,
            None => return Ok(None),
        };
        if self.estimated_tokens(observation) <= limit {
            return Ok(None);
        }
        let tool_id =
            Self::tool_call_id_from_action(action).unwrap_or_else(|| "unknown".to_string());
        let key = format!("{}", tool_id);
        let full_value = json!(observation);
        runtime
            .store()
            .put(TOOL_EVICTION_NAMESPACE, &key, full_value)
            .await;

        let preview = if observation.chars().count() <= self.preview_chars {
            observation.to_string()
        } else {
            let trimmed: String = observation.chars().take(self.preview_chars).collect();
            format!("{}\n\n... [truncated]", trimmed)
        };
        let notice = format!(
            "\n\n[Full output ({} chars) written to store key tool_eviction/{}]. You can read it via the store if needed.",
            observation.len(),
            key
        );
        Ok(Some(format!("{}{}", preview, notice)))
    }
}
