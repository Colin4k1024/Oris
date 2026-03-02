//! Replay cursor engine: reconstruct state from the event log without live side effects.
//!
//! The replay loop is: **Load checkpoint → Replay events → Inject recorded outputs → Reconstruct state.**
//! This engine holds only the event store, optional snapshot store, and reducer. It has **no**
//! executor or step function, so **live tool execution is hard-disabled** during replay.
//! Recorded action outputs (ActionSucceeded/ActionFailed) are already in the log and applied by the reducer.

use crate::kernel::event::SequencedEvent;
use crate::kernel::identity::{RunId, Seq};
use crate::kernel::reducer::Reducer;
use crate::kernel::snapshot::{Snapshot, SnapshotStore};
use crate::kernel::state::KernelState;
use crate::kernel::EventStore;
use crate::kernel::KernelError;

/// Replay cursor: reconstructs state from the event log. No executor, no live execution.
///
/// Use this when you need to replay a run (e.g. verify, debug, or resume). The cursor
/// loads the latest checkpoint if available, then applies all subsequent events in order.
/// Action results are taken from the log (ActionSucceeded/ActionFailed), not from live execution.
pub struct ReplayCursor<S: KernelState> {
    /// Event store (source of truth).
    pub events: Box<dyn EventStore>,
    /// Optional snapshot store for checkpoint-based replay (optimization).
    pub snaps: Option<Box<dyn SnapshotStore<S>>>,
    /// Reducer to apply each event to state (deterministic).
    pub reducer: Box<dyn Reducer<S>>,
}

impl<S: KernelState> ReplayCursor<S> {
    /// Replays the run: load checkpoint (if any) → replay events → return final state.
    ///
    /// Recorded outputs are injected via the event log (reducer applies ActionSucceeded/ActionFailed).
    /// No live tool or LLM execution occurs.
    pub fn replay(&self, run_id: &RunId, initial_state: S) -> Result<S, KernelError> {
        self.replay_from(run_id, initial_state, None)
    }

    /// Replays from an optional snapshot. If `snapshot` is provided, state starts at
    /// `snapshot.state` and only events with seq > snapshot.at_seq are applied.
    pub fn replay_from(
        &self,
        run_id: &RunId,
        initial_state: S,
        snapshot: Option<&Snapshot<S>>,
    ) -> Result<S, KernelError> {
        const FROM_SEQ: Seq = 1;
        let (mut state, from_seq) = match snapshot {
            Some(snap) => (snap.state.clone(), snap.at_seq + 1),
            None => (initial_state, FROM_SEQ),
        };
        let sequenced = self.events.scan(run_id, from_seq)?;
        for se in sequenced {
            self.reducer.apply(&mut state, &se)?;
        }
        Ok(state)
    }

    /// Replays using the latest snapshot from the snapshot store (if any).
    /// Equivalent to load checkpoint → replay events → reconstruct state.
    pub fn replay_from_checkpoint(
        &self,
        run_id: &RunId,
        initial_state: S,
    ) -> Result<S, KernelError> {
        let from_snap = self
            .snaps
            .as_ref()
            .and_then(|s| s.load_latest(run_id).ok().flatten());
        self.replay_from(run_id, initial_state, from_snap.as_ref())
    }

    /// Replays event-by-event, yielding state after each applied event (for debugging/verify).
    /// No live execution; each event is applied via the reducer only.
    pub fn replay_step<'a>(
        &'a self,
        run_id: &RunId,
        initial_state: S,
        from_seq: Seq,
    ) -> Result<ReplayStepIter<'a, S>, KernelError> {
        let sequenced = self.events.scan(run_id, from_seq)?;
        Ok(ReplayStepIter {
            state: initial_state,
            events: sequenced,
            reducer: self.reducer.as_ref(),
            index: 0,
        })
    }
}

/// Iterator that yields (state_after_event, sequenced_event) when replaying step-by-step.
pub struct ReplayStepIter<'a, S: KernelState> {
    state: S,
    events: Vec<SequencedEvent>,
    reducer: &'a dyn Reducer<S>,
    index: usize,
}

impl<'a, S: KernelState> ReplayStepIter<'a, S> {
    /// Returns the current state and the next event index (1-based from run), or None if done.
    pub fn next_step(&mut self) -> Result<Option<(S, SequencedEvent)>, KernelError> {
        let se = match self.events.get(self.index) {
            Some(se) => se.clone(),
            None => return Ok(None),
        };
        self.reducer.apply(&mut self.state, &se)?;
        self.index += 1;
        Ok(Some((self.state.clone(), se)))
    }

    /// Current state (after all steps applied so far).
    pub fn current_state(&self) -> &S {
        &self.state
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kernel::event::Event;
    use crate::kernel::event_store::InMemoryEventStore;
    use crate::kernel::StateUpdatedOnlyReducer;
    use serde::{Deserialize, Serialize};

    #[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
    struct TestState(u32);

    impl crate::kernel::state::KernelState for TestState {
        fn version(&self) -> u32 {
            1
        }
    }

    #[test]
    fn replay_from_scratch_reconstructs_state() {
        let events = InMemoryEventStore::new();
        let run_id: RunId = "r1".into();
        events
            .append(
                &run_id,
                &[
                    Event::StateUpdated {
                        step_id: Some("a".into()),
                        payload: serde_json::to_value(&TestState(1)).unwrap(),
                    },
                    Event::StateUpdated {
                        step_id: Some("b".into()),
                        payload: serde_json::to_value(&TestState(2)).unwrap(),
                    },
                    Event::Completed,
                ],
            )
            .unwrap();
        let cursor = ReplayCursor::<TestState> {
            events: Box::new(events),
            snaps: None,
            reducer: Box::new(StateUpdatedOnlyReducer),
        };
        let state = cursor.replay(&run_id, TestState(0)).unwrap();
        assert_eq!(state, TestState(2));
    }

    #[test]
    fn replay_from_checkpoint_applies_only_tail() {
        let events = InMemoryEventStore::new();
        let run_id: RunId = "r2".into();
        events
            .append(
                &run_id,
                &[
                    Event::StateUpdated {
                        step_id: Some("a".into()),
                        payload: serde_json::to_value(&TestState(10)).unwrap(),
                    },
                    Event::StateUpdated {
                        step_id: Some("b".into()),
                        payload: serde_json::to_value(&TestState(20)).unwrap(),
                    },
                    Event::Completed,
                ],
            )
            .unwrap();
        let snaps = crate::kernel::snapshot::InMemorySnapshotStore::new();
        snaps
            .save(&Snapshot {
                run_id: run_id.clone(),
                at_seq: 1,
                state: TestState(10),
            })
            .unwrap();
        let cursor = ReplayCursor::<TestState> {
            events: Box::new(events),
            snaps: Some(Box::new(snaps)),
            reducer: Box::new(StateUpdatedOnlyReducer),
        };
        let state = cursor
            .replay_from_checkpoint(&run_id, TestState(0))
            .unwrap();
        assert_eq!(state, TestState(20));
    }

    #[test]
    fn replay_step_yields_state_after_each_event() {
        let events = InMemoryEventStore::new();
        let run_id: RunId = "r3".into();
        events
            .append(
                &run_id,
                &[
                    Event::StateUpdated {
                        step_id: Some("a".into()),
                        payload: serde_json::to_value(&TestState(1)).unwrap(),
                    },
                    Event::StateUpdated {
                        step_id: Some("b".into()),
                        payload: serde_json::to_value(&TestState(2)).unwrap(),
                    },
                ],
            )
            .unwrap();
        let cursor = ReplayCursor::<TestState> {
            events: Box::new(events),
            snaps: None,
            reducer: Box::new(StateUpdatedOnlyReducer),
        };
        let mut iter = cursor.replay_step(&run_id, TestState(0), 1).unwrap();
        let (s1, _) = iter.next_step().unwrap().unwrap();
        assert_eq!(s1, TestState(1));
        let (s2, _) = iter.next_step().unwrap().unwrap();
        assert_eq!(s2, TestState(2));
        assert!(iter.next_step().unwrap().is_none());
    }
}
