//! Snapshot and SnapshotStore for the Oris kernel.
//!
//! **Checkpointing/snapshots are strictly an optimization layer.** The source of truth
//! is the event-sourced execution log (see [crate::kernel::execution_log::ExecutionLog] and
//! [crate::kernel::event::EventStore]). Snapshots only speed up replay by providing
//! initial state at a given seq; they do not replace the log.
//! Every snapshot must carry `at_seq` (the seq up to which state has been projected).

use serde::{Deserialize, Serialize};

use crate::kernel::identity::{RunId, Seq};
use crate::kernel::KernelError;

/// A snapshot of state at a given sequence number.
///
/// **Invariant:** `at_seq` is the seq of the last event that was applied to produce this state.
/// Recovery: load latest snapshot, then replay events with seq > at_seq.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Snapshot<S> {
    /// Run this snapshot belongs to.
    pub run_id: RunId,
    /// Sequence number of the last event applied (projection point).
    pub at_seq: Seq,
    /// The state at this point.
    pub state: S,
}

/// Snapshot store: load latest snapshot or save a new one (optimization layer).
pub trait SnapshotStore<S>: Send + Sync {
    /// Loads the latest snapshot for the run, if any.
    fn load_latest(&self, run_id: &RunId) -> Result<Option<Snapshot<S>>, KernelError>;

    /// Saves a snapshot. Overwrites or appends according to implementation.
    fn save(&self, snapshot: &Snapshot<S>) -> Result<(), KernelError>;
}

/// In-memory snapshot store: one snapshot per run (latest overwrites).
pub struct InMemorySnapshotStore<S> {
    latest: std::sync::RwLock<std::collections::HashMap<RunId, Snapshot<S>>>,
}

impl<S: Clone + Send + Sync> InMemorySnapshotStore<S> {
    pub fn new() -> Self {
        Self {
            latest: std::sync::RwLock::new(std::collections::HashMap::new()),
        }
    }
}

impl<S: Clone + Send + Sync> Default for InMemorySnapshotStore<S> {
    fn default() -> Self {
        Self::new()
    }
}

impl<S: Clone + Send + Sync> SnapshotStore<S> for InMemorySnapshotStore<S> {
    fn load_latest(&self, run_id: &RunId) -> Result<Option<Snapshot<S>>, KernelError> {
        let guard = self
            .latest
            .read()
            .map_err(|e| KernelError::SnapshotStore(e.to_string()))?;
        Ok(guard.get(run_id).cloned())
    }

    fn save(&self, snapshot: &Snapshot<S>) -> Result<(), KernelError> {
        let mut guard = self
            .latest
            .write()
            .map_err(|e| KernelError::SnapshotStore(e.to_string()))?;
        guard.insert(snapshot.run_id.clone(), snapshot.clone());
        Ok(())
    }
}
