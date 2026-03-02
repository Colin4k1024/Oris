//! Canonical execution log: event-sourced log as the source of truth.
//!
//! [ExecutionLog] is the canonical record type for one entry in the execution log.
//! The event store holds the log; snapshots/checkpoints are strictly an optimization
//! for replay (see [crate::kernel::snapshot]).

use serde::{Deserialize, Serialize};

use crate::kernel::event::Event;
use crate::kernel::identity::{RunId, Seq, StepId};

/// One canonical execution log entry: thread (run), step, index, event, and optional state hash.
///
/// The event log is the source of truth. Checkpointing/snapshots are used only to
/// speed up replay by providing initial state at a given seq; they do not replace
/// the log.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExecutionLog {
    /// Run (thread) this entry belongs to.
    pub thread_id: RunId,
    /// Step identifier when the event is associated with a step (e.g. from StateUpdated).
    pub step_id: Option<StepId>,
    /// Monotonic event index (sequence number) within the run.
    pub event_index: Seq,
    /// The event at this index.
    pub event: Event,
    /// Optional hash of state after applying this event (for verification/replay).
    pub state_hash: Option<[u8; 32]>,
}

impl ExecutionLog {
    /// Builds an execution log entry from a sequenced event and run id.
    /// `state_hash` is optional (e.g. when reading from store without reducer).
    pub fn from_sequenced(
        thread_id: RunId,
        se: &crate::kernel::event::SequencedEvent,
        state_hash: Option<[u8; 32]>,
    ) -> Self {
        let step_id = step_id_from_event(&se.event);
        Self {
            thread_id,
            step_id,
            event_index: se.seq,
            event: se.event.clone(),
            state_hash,
        }
    }
}

/// Extracts step_id from the event when present (e.g. StateUpdated).
fn step_id_from_event(event: &Event) -> Option<StepId> {
    match event {
        Event::StateUpdated { step_id, .. } => step_id.clone(),
        _ => None,
    }
}

/// Scans the event store for the run and returns the canonical execution log (state_hash None).
pub fn scan_execution_log(
    store: &dyn crate::kernel::event::EventStore,
    run_id: &RunId,
    from: Seq,
) -> Result<Vec<ExecutionLog>, crate::kernel::KernelError> {
    let sequenced = store.scan(run_id, from)?;
    Ok(sequenced
        .iter()
        .map(|se| ExecutionLog::from_sequenced(run_id.clone(), se, None))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kernel::event::{EventStore, SequencedEvent};
    use crate::kernel::event_store::InMemoryEventStore;

    #[test]
    fn scan_execution_log_returns_canonical_entries() {
        let store = InMemoryEventStore::new();
        let run_id: RunId = "run-scan".into();
        store
            .append(
                &run_id,
                &[
                    Event::StateUpdated {
                        step_id: Some("n1".into()),
                        payload: serde_json::json!([1]),
                    },
                    Event::Completed,
                ],
            )
            .unwrap();
        let log = scan_execution_log(&store, &run_id, 1).unwrap();
        assert_eq!(log.len(), 2);
        assert_eq!(log[0].thread_id, run_id);
        assert_eq!(log[0].event_index, 1);
        assert_eq!(log[0].step_id.as_deref(), Some("n1"));
        assert_eq!(log[1].event_index, 2);
        assert!(matches!(log[1].event, Event::Completed));
    }

    #[test]
    fn from_sequenced_state_updated_has_step_id() {
        let thread_id: RunId = "run-1".into();
        let se = SequencedEvent {
            seq: 1,
            event: Event::StateUpdated {
                step_id: Some("node-a".into()),
                payload: serde_json::json!([1]),
            },
        };
        let log = ExecutionLog::from_sequenced(thread_id.clone(), &se, None);
        assert_eq!(log.thread_id, thread_id);
        assert_eq!(log.step_id.as_deref(), Some("node-a"));
        assert_eq!(log.event_index, 1);
        assert!(log.state_hash.is_none());
    }

    #[test]
    fn from_sequenced_completed_has_no_step_id() {
        let thread_id: RunId = "run-2".into();
        let se = SequencedEvent {
            seq: 2,
            event: Event::Completed,
        };
        let log = ExecutionLog::from_sequenced(thread_id.clone(), &se, None);
        assert_eq!(log.step_id, None);
        assert_eq!(log.event_index, 2);
    }
}
