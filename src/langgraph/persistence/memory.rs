use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::RwLock;

#[cfg(feature = "uuid")]
use uuid::Uuid;

use crate::langgraph::state::State;

use super::{checkpointer::Checkpointer, error::PersistenceError, snapshot::StateSnapshot};

/// In-memory checkpointer implementation
///
/// This is useful for development and testing. Checkpoints are stored
/// in memory and will be lost when the process exits.
pub struct InMemorySaver<S: State> {
    checkpoints: Arc<RwLock<HashMap<String, Vec<StateSnapshot<S>>>>>,
}

impl<S: State> InMemorySaver<S> {
    /// Create a new InMemorySaver
    pub fn new() -> Self {
        Self {
            checkpoints: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

impl<S: State> Default for InMemorySaver<S> {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl<S: State> Checkpointer<S> for InMemorySaver<S> {
    async fn put(
        &self,
        thread_id: &str,
        checkpoint: &StateSnapshot<S>,
    ) -> Result<String, PersistenceError> {
        let checkpoint_id = checkpoint.checkpoint_id().cloned().unwrap_or_else(|| {
            #[cfg(feature = "uuid")]
            {
                Uuid::new_v4().to_string()
            }
            #[cfg(not(feature = "uuid"))]
            {
                use std::time::{SystemTime, UNIX_EPOCH};
                format!(
                    "checkpoint-{}",
                    SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap()
                        .as_nanos()
                )
            }
        });

        let mut checkpoints = self.checkpoints.write().await;

        // Create a new snapshot with the checkpoint_id
        let mut new_checkpoint = checkpoint.clone();
        new_checkpoint.config.checkpoint_id = Some(checkpoint_id.clone());

        // Get or create the thread's checkpoint list
        let thread_checkpoints = checkpoints
            .entry(thread_id.to_string())
            .or_insert_with(Vec::new);

        // Add the checkpoint
        thread_checkpoints.push(new_checkpoint);

        Ok(checkpoint_id)
    }

    async fn get(
        &self,
        thread_id: &str,
        checkpoint_id: Option<&str>,
    ) -> Result<Option<StateSnapshot<S>>, PersistenceError> {
        let checkpoints = self.checkpoints.read().await;

        let thread_checkpoints = match checkpoints.get(thread_id) {
            Some(cps) => cps,
            None => return Ok(None),
        };

        let result = if let Some(_cp_id) = checkpoint_id {
            // Find specific checkpoint
            thread_checkpoints
                .iter()
                .find(|cp| match (cp.checkpoint_id().as_deref(), checkpoint_id) {
                    (Some(a), Some(b)) => a == b,
                    (None, None) => true,
                    _ => false,
                })
                .cloned()
        } else {
            // Return latest checkpoint
            thread_checkpoints.last().cloned()
        };

        Ok(result)
    }

    async fn list(
        &self,
        thread_id: &str,
        limit: Option<usize>,
    ) -> Result<Vec<StateSnapshot<S>>, PersistenceError> {
        let checkpoints = self.checkpoints.read().await;

        let thread_checkpoints = match checkpoints.get(thread_id) {
            Some(cps) => cps,
            None => return Ok(Vec::new()),
        };

        let mut result: Vec<StateSnapshot<S>> = thread_checkpoints.clone();

        // Apply limit if specified
        if let Some(limit) = limit {
            let len = result.len();
            if len > limit {
                result.drain(0..(len - limit));
            }
        }

        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::super::config::CheckpointConfig;
    use super::*;
    use crate::langgraph::state::MessagesState;

    #[tokio::test]
    async fn test_in_memory_saver() {
        let saver = InMemorySaver::<MessagesState>::new();
        let state = MessagesState::new();
        let config = CheckpointConfig::new("thread-1");
        let snapshot = StateSnapshot::new(state, vec!["node1".to_string()], config);

        let checkpoint_id = saver.put("thread-1", &snapshot).await.unwrap();
        assert!(!checkpoint_id.is_empty());

        let retrieved = saver.get("thread-1", None).await.unwrap();
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().thread_id(), "thread-1");

        let list = saver.list("thread-1", None).await.unwrap();
        assert_eq!(list.len(), 1);
    }
}
