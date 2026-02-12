use crate::langgraph::{
    error::LangGraphError,
    persistence::{checkpointer::CheckpointerBox, snapshot::StateSnapshot},
    state::State,
};

/// Durability mode for checkpoint saving
///
/// Determines when and how checkpoints are saved during graph execution.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DurabilityMode {
    /// Save checkpoint only when graph execution exits
    ///
    /// This provides the best performance but means intermediate state
    /// is not saved, so you cannot recover from mid-execution failures.
    Exit,

    /// Save checkpoints asynchronously
    ///
    /// Checkpoints are saved in the background without blocking execution.
    /// This provides good performance and durability, but there's a small
    /// risk that checkpoints may not be written if the process crashes.
    Async,

    /// Save checkpoints synchronously
    ///
    /// Checkpoints are saved before the next step starts, ensuring
    /// high durability at the cost of some performance overhead.
    Sync,
}

impl Default for DurabilityMode {
    fn default() -> Self {
        Self::Sync
    }
}

impl DurabilityMode {
    /// Parse durability mode from string
    pub fn from_str(s: &str) -> Result<Self, LangGraphError> {
        match s.to_lowercase().as_str() {
            "exit" => Ok(Self::Exit),
            "async" => Ok(Self::Async),
            "sync" => Ok(Self::Sync),
            _ => Err(LangGraphError::ExecutionError(format!(
                "Invalid durability mode: {}. Must be one of: exit, async, sync",
                s
            ))),
        }
    }
}

/// Save a checkpoint according to the durability mode
pub async fn save_checkpoint<S: State + 'static>(
    checkpointer: Option<&CheckpointerBox<S>>,
    snapshot: &StateSnapshot<S>,
    mode: DurabilityMode,
) -> Result<(), LangGraphError> {
    if let Some(checkpointer) = checkpointer {
        match mode {
            DurabilityMode::Exit => {
                // Don't save here, will be saved on exit
                Ok(())
            }
            DurabilityMode::Async => {
                // Spawn async task to save checkpoint
                let checkpointer = checkpointer.clone();
                let snapshot = snapshot.clone();
                let thread_id = snapshot.thread_id().to_string();

                tokio::spawn(async move {
                    if let Err(e) = checkpointer.put(&thread_id, &snapshot).await {
                        log::error!("Failed to save checkpoint asynchronously: {}", e);
                    }
                });

                Ok(())
            }
            DurabilityMode::Sync => {
                // Save synchronously
                checkpointer
                    .put(snapshot.thread_id(), snapshot)
                    .await
                    .map_err(|e| {
                        LangGraphError::ExecutionError(format!("Failed to save checkpoint: {}", e))
                    })?;
                Ok(())
            }
        }
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_durability_mode_from_str() {
        assert_eq!(
            DurabilityMode::from_str("exit").unwrap(),
            DurabilityMode::Exit
        );
        assert_eq!(
            DurabilityMode::from_str("async").unwrap(),
            DurabilityMode::Async
        );
        assert_eq!(
            DurabilityMode::from_str("sync").unwrap(),
            DurabilityMode::Sync
        );
        assert!(DurabilityMode::from_str("invalid").is_err());
    }
}
