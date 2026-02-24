use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Configuration for graph execution with persistence
///
/// Similar to Python's RunnableConfig, this contains configurable
/// parameters like thread_id and checkpoint_id.
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct RunnableConfig {
    /// Configurable parameters (thread_id, checkpoint_id, etc.)
    pub configurable: HashMap<String, Value>,
}

impl RunnableConfig {
    /// Create a new RunnableConfig
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a config with thread_id
    pub fn with_thread_id(thread_id: impl Into<String>) -> Self {
        let mut config = Self::new();
        config
            .configurable
            .insert("thread_id".to_string(), Value::String(thread_id.into()));
        config
    }

    /// Create a config with thread_id and checkpoint_id
    pub fn with_checkpoint(thread_id: impl Into<String>, checkpoint_id: impl Into<String>) -> Self {
        let mut config = Self::with_thread_id(thread_id);
        config.configurable.insert(
            "checkpoint_id".to_string(),
            Value::String(checkpoint_id.into()),
        );
        config
    }

    /// Get thread_id from config
    pub fn get_thread_id(&self) -> Option<String> {
        self.configurable
            .get("thread_id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
    }

    /// Get checkpoint_id from config
    pub fn get_checkpoint_id(&self) -> Option<String> {
        self.configurable
            .get("checkpoint_id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
    }

    /// Get checkpoint_ns from config
    pub fn get_checkpoint_ns(&self) -> Option<String> {
        self.configurable
            .get("checkpoint_ns")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
    }

    /// Get user_id from config (for store)
    pub fn get_user_id(&self) -> Option<String> {
        self.configurable
            .get("user_id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
    }

    /// When true, allows step_once to run on a graph marked non-pure (with_pure_guard(false)).
    /// Default is false; set to true only for compatibility when nodes perform I/O until refactored to Actions.
    pub fn allow_non_pure_step_once(&self) -> bool {
        self.configurable
            .get("allow_non_pure_step_once")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
    }

    /// Set allow_non_pure_step_once for step_once guard compatibility.
    pub fn with_allow_non_pure_step_once(mut self, allow: bool) -> Self {
        self.configurable.insert(
            "allow_non_pure_step_once".to_string(),
            Value::Bool(allow),
        );
        self
    }
}

/// Checkpoint configuration
///
/// Contains information about a checkpoint including thread_id,
/// checkpoint_id, and optional namespace.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct CheckpointConfig {
    pub thread_id: String,
    pub checkpoint_id: Option<String>,
    pub checkpoint_ns: Option<String>,
}

impl CheckpointConfig {
    /// Create a new CheckpointConfig
    pub fn new(thread_id: impl Into<String>) -> Self {
        Self {
            thread_id: thread_id.into(),
            checkpoint_id: None,
            checkpoint_ns: None,
        }
    }

    /// Create from RunnableConfig
    pub fn from_config(config: &RunnableConfig) -> Result<Self, crate::graph::error::GraphError> {
        let thread_id = config.get_thread_id().ok_or_else(|| {
            crate::graph::error::GraphError::ExecutionError(
                "thread_id is required in config".to_string(),
            )
        })?;

        Ok(Self {
            thread_id,
            checkpoint_id: config.get_checkpoint_id(),
            checkpoint_ns: config.get_checkpoint_ns(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_runnable_config() {
        let config = RunnableConfig::with_thread_id("thread-1");
        assert_eq!(config.get_thread_id(), Some("thread-1".to_string()));
        assert_eq!(config.get_checkpoint_id(), None);

        let config = RunnableConfig::with_checkpoint("thread-1", "checkpoint-1");
        assert_eq!(config.get_thread_id(), Some("thread-1".to_string()));
        assert_eq!(config.get_checkpoint_id(), Some("checkpoint-1".to_string()));
    }

    #[test]
    fn test_checkpoint_config() {
        let runnable_config = RunnableConfig::with_checkpoint("thread-1", "checkpoint-1");
        let checkpoint_config = CheckpointConfig::from_config(&runnable_config).unwrap();
        assert_eq!(checkpoint_config.thread_id, "thread-1");
        assert_eq!(
            checkpoint_config.checkpoint_id,
            Some("checkpoint-1".to_string())
        );
    }
}
