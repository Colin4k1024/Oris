//! Canonical execution log: event-sourced log as the source of truth.
//!
//! [ExecutionLog] is the canonical record type for one entry in the execution log.
//! The event store holds the log; snapshots/checkpoints are strictly an optimization
//! for replay (see [crate::kernel::snapshot]).

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::kernel::event::Event;
use crate::kernel::identity::{RunId, Seq, StepId};
use crate::kernel::reducer::Reducer;
use crate::kernel::state::KernelState;

/// Unified kernel trace event shape for audit and observability consumers.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct KernelTraceEvent {
    pub run_id: RunId,
    pub seq: Seq,
    pub step_id: Option<StepId>,
    pub action_id: Option<String>,
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp_ms: Option<i64>,
}

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

    pub fn to_trace_event(&self) -> KernelTraceEvent {
        KernelTraceEvent {
            run_id: self.thread_id.clone(),
            seq: self.event_index,
            step_id: self.step_id.clone(),
            action_id: action_id_from_event(&self.event),
            kind: event_kind(&self.event),
            timestamp_ms: None,
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

fn action_id_from_event(event: &Event) -> Option<String> {
    match event {
        Event::ActionRequested { action_id, .. }
        | Event::ActionSucceeded { action_id, .. }
        | Event::ActionFailed { action_id, .. } => Some(action_id.clone()),
        _ => None,
    }
}

fn event_kind(event: &Event) -> String {
    match event {
        Event::StateUpdated { .. } => "StateUpdated".into(),
        Event::ActionRequested { .. } => "ActionRequested".into(),
        Event::ActionSucceeded { .. } => "ActionSucceeded".into(),
        Event::ActionFailed { .. } => "ActionFailed".into(),
        Event::Interrupted { .. } => "Interrupted".into(),
        Event::Resumed { .. } => "Resumed".into(),
        Event::Completed => "Completed".into(),
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

/// Reconstructs the execution log and attaches a deterministic state hash after each event.
///
/// `initial_state` must represent the state immediately before `from`. Callers replaying from
/// a checkpoint should pass the checkpointed state and the next sequence number.
pub(crate) fn scan_execution_log_with_state_hashes<S>(
    store: &dyn crate::kernel::event::EventStore,
    run_id: &RunId,
    from: Seq,
    initial_state: S,
    reducer: &dyn Reducer<S>,
) -> Result<Vec<ExecutionLog>, crate::kernel::KernelError>
where
    S: KernelState + Serialize,
{
    let sequenced = store.scan(run_id, from)?;
    let mut state = initial_state;
    let mut out = Vec::with_capacity(sequenced.len());
    for se in sequenced {
        reducer.apply(&mut state, &se)?;
        out.push(ExecutionLog::from_sequenced(
            run_id.clone(),
            &se,
            Some(state_hash(&state)?),
        ));
    }
    Ok(out)
}

/// Scans the event store and returns the unified kernel trace event view.
pub fn scan_execution_trace(
    store: &dyn crate::kernel::event::EventStore,
    run_id: &RunId,
    from: Seq,
) -> Result<Vec<KernelTraceEvent>, crate::kernel::KernelError> {
    Ok(scan_execution_log(store, run_id, from)?
        .into_iter()
        .map(|entry| entry.to_trace_event())
        .collect())
}

fn state_hash<S: Serialize>(state: &S) -> Result<[u8; 32], crate::kernel::KernelError> {
    let canonical = serde_json::to_vec(state)
        .map_err(|e| crate::kernel::KernelError::Driver(format!("serialize state hash: {}", e)))?;
    let mut hasher = Sha256::new();
    hasher.update(canonical);
    Ok(hasher.finalize().into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kernel::event::{EventStore, SequencedEvent};
    use crate::kernel::event_store::InMemoryEventStore;
    use crate::kernel::StateUpdatedOnlyReducer;
    use serde::{Deserialize, Serialize};

    #[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
    struct TestState(u32);

    impl crate::kernel::KernelState for TestState {
        fn version(&self) -> u32 {
            1
        }
    }

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

    #[test]
    fn execution_log_converts_to_kernel_trace_event() {
        let log = ExecutionLog {
            thread_id: "run-trace".into(),
            step_id: Some("node-a".into()),
            event_index: 3,
            event: Event::ActionRequested {
                action_id: "a1".into(),
                payload: serde_json::json!({"tool": "demo"}),
            },
            state_hash: None,
        };

        let trace = log.to_trace_event();
        assert_eq!(trace.run_id, "run-trace");
        assert_eq!(trace.seq, 3);
        assert_eq!(trace.step_id.as_deref(), Some("node-a"));
        assert_eq!(trace.action_id.as_deref(), Some("a1"));
        assert_eq!(trace.kind, "ActionRequested");
        assert_eq!(trace.timestamp_ms, None);
    }

    #[test]
    fn scan_execution_log_with_state_hashes_populates_hashes() {
        let store = InMemoryEventStore::new();
        let run_id: RunId = "run-hash".into();
        store
            .append(
                &run_id,
                &[
                    Event::StateUpdated {
                        step_id: Some("n1".into()),
                        payload: serde_json::to_value(&TestState(1)).unwrap(),
                    },
                    Event::Completed,
                ],
            )
            .unwrap();

        let log = scan_execution_log_with_state_hashes(
            &store,
            &run_id,
            1,
            TestState(0),
            &StateUpdatedOnlyReducer,
        )
        .unwrap();
        assert_eq!(log.len(), 2);
        assert!(log[0].state_hash.is_some());
        assert_eq!(
            log[0].state_hash, log[1].state_hash,
            "Completed should preserve the last projected state hash"
        );
    }
}
