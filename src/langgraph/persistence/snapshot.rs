use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::langgraph::state::State;

use super::config::CheckpointConfig;

/// State snapshot - a checkpoint of graph state at a particular point in time
///
/// Similar to Python's StateSnapshot, this contains the state values,
/// next nodes to execute, configuration, and metadata.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(bound = "S: Serialize + serde::de::DeserializeOwned")]
pub struct StateSnapshot<S: State> {
    /// State values at this checkpoint
    pub values: S,
    /// Next nodes to execute
    pub next: Vec<String>,
    /// Configuration for this checkpoint
    pub config: CheckpointConfig,
    /// Metadata associated with this checkpoint
    pub metadata: HashMap<String, Value>,
    /// Timestamp when this checkpoint was created
    pub created_at: DateTime<Utc>,
    /// Parent checkpoint configuration (for forking/replay)
    pub parent_config: Option<CheckpointConfig>,
}

impl<S: State> StateSnapshot<S> {
    /// Create a new StateSnapshot
    pub fn new(values: S, next: Vec<String>, config: CheckpointConfig) -> Self {
        Self {
            values,
            next,
            config,
            metadata: HashMap::new(),
            created_at: Utc::now(),
            parent_config: None,
        }
    }

    /// Create a snapshot with metadata
    pub fn with_metadata(
        values: S,
        next: Vec<String>,
        config: CheckpointConfig,
        metadata: HashMap<String, Value>,
    ) -> Self {
        Self {
            values,
            next,
            config,
            metadata,
            created_at: Utc::now(),
            parent_config: None,
        }
    }

    /// Create a snapshot with parent config (for forking)
    pub fn with_parent(
        values: S,
        next: Vec<String>,
        config: CheckpointConfig,
        parent_config: CheckpointConfig,
    ) -> Self {
        Self {
            values,
            next,
            config,
            metadata: HashMap::new(),
            created_at: Utc::now(),
            parent_config: Some(parent_config),
        }
    }

    /// Get the checkpoint ID
    pub fn checkpoint_id(&self) -> Option<&String> {
        self.config.checkpoint_id.as_ref()
    }

    /// Get the thread ID
    pub fn thread_id(&self) -> &str {
        &self.config.thread_id
    }

    /// Convert to RunnableConfig for resuming execution
    ///
    /// Creates a new RunnableConfig with the thread_id and checkpoint_id
    /// from this snapshot. This is useful for time-travel scenarios where
    /// you want to resume execution from a specific checkpoint.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// let history = graph.get_state_history(&config).await?;
    /// let selected_checkpoint = &history[1];
    /// let new_config = selected_checkpoint.to_config();
    /// let resumed = graph.invoke_with_config(None, &new_config).await?;
    /// ```
    pub fn to_config(&self) -> super::config::RunnableConfig {
        let mut config = super::config::RunnableConfig::with_thread_id(self.thread_id());
        if let Some(checkpoint_id) = self.checkpoint_id() {
            config.configurable.insert(
                "checkpoint_id".to_string(),
                serde_json::json!(checkpoint_id),
            );
        }
        if let Some(checkpoint_ns) = &self.config.checkpoint_ns {
            config.configurable.insert(
                "checkpoint_ns".to_string(),
                serde_json::json!(checkpoint_ns),
            );
        }
        config
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::langgraph::state::MessagesState;

    #[test]
    fn test_state_snapshot() {
        let state = MessagesState::new();
        let config = CheckpointConfig::new("thread-1");
        let snapshot = StateSnapshot::new(state, vec!["node1".to_string()], config);

        assert_eq!(snapshot.thread_id(), "thread-1");
        assert_eq!(snapshot.next, vec!["node1"]);
    }
}
