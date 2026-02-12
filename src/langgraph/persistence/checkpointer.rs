use std::sync::Arc;

use async_trait::async_trait;

use crate::langgraph::state::State;

use super::{error::PersistenceError, snapshot::StateSnapshot};

/// Trait for checkpoint savers
///
/// Checkpointers save and retrieve state snapshots for graph execution.
/// This allows for persistence, replay, and fault tolerance.
#[async_trait]
pub trait Checkpointer<S: State>: Send + Sync {
    /// Save a checkpoint
    ///
    /// Returns the checkpoint_id of the saved checkpoint.
    async fn put(
        &self,
        thread_id: &str,
        checkpoint: &StateSnapshot<S>,
    ) -> Result<String, PersistenceError>;

    /// Get a checkpoint
    ///
    /// If checkpoint_id is None, returns the latest checkpoint for the thread.
    async fn get(
        &self,
        thread_id: &str,
        checkpoint_id: Option<&str>,
    ) -> Result<Option<StateSnapshot<S>>, PersistenceError>;

    /// List checkpoints for a thread
    ///
    /// Returns checkpoints in chronological order (oldest first).
    /// If limit is specified, returns only the most recent N checkpoints.
    async fn list(
        &self,
        thread_id: &str,
        limit: Option<usize>,
    ) -> Result<Vec<StateSnapshot<S>>, PersistenceError>;
}

/// Type alias for a boxed checkpointer
pub type CheckpointerBox<S> = Arc<dyn Checkpointer<S>>;
