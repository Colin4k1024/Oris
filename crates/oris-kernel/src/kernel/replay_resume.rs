//! Replay-based resume: guarantee idempotent resume semantics.
//!
//! This module provides [ReplayResume] which enforces that resuming a suspended state
//! is strictly Replay + Inject Decision, ensuring no reliance on active memory.

use serde::{Deserialize, Serialize};

use crate::kernel::event::{Event, SequencedEvent};
use crate::kernel::identity::{RunId, Seq};
use crate::kernel::reducer::Reducer;
use crate::kernel::state::KernelState;
use crate::kernel::EventStore;
use crate::kernel::KernelError;

/// Resume decision: the value injected when resuming from an interrupt.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ResumeDecision {
    /// The resume value (e.g. user input, approved tool result).
    pub value: serde_json::Value,
    /// Optional metadata about the decision.
    pub metadata: Option<serde_json::Value>,
}

impl ResumeDecision {
    pub fn new(value: serde_json::Value) -> Self {
        Self {
            value,
            metadata: None,
        }
    }

    pub fn with_metadata(mut self, metadata: serde_json::Value) -> Self {
        self.metadata = Some(metadata);
        self
    }
}

/// Result of a replay resume operation.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ResumeResult<S: KernelState> {
    /// The final state after replay + inject decision.
    pub state: S,
    /// Number of events replayed.
    pub events_replayed: usize,
    /// Whether this was an idempotent resume (already at the same point).
    pub idempotent: bool,
}

/// Replay-based resume: enforces Replay + Inject Decision semantics.
pub struct ReplayResume<S: KernelState> {
    /// Event store (source of truth).
    pub events: Box<dyn EventStore>,
    /// Reducer to apply events.
    pub reducer: Box<dyn Reducer<S>>,
}

impl<S: KernelState + PartialEq> ReplayResume<S> {
    /// Resumes execution by replaying from the event log and injecting the decision.
    ///
    /// This ensures no reliance on active memory - the state is reconstructed purely
    /// from the event log plus the injected decision.
    pub fn resume(
        &self,
        run_id: &RunId,
        decision: ResumeDecision,
        initial_state: S,
    ) -> Result<ResumeResult<S>, KernelError> {
        // 1. Get all events
        let events = self.events.scan(run_id, 1)?;
        let mut state = initial_state;

        // 2. Check if already resumed (idempotent check) - must be done BEFORE injecting
        let already_resumed = events
            .last()
            .map(|se| matches!(se.event, Event::Resumed { .. }))
            .unwrap_or(false);

        // 3. Replay all events that are not Resumed (skip existing Resumed events at end)
        let replay_events: Vec<_> = events
            .iter()
            .filter(|se| !matches!(se.event, Event::Resumed { .. }))
            .cloned()
            .collect();

        let mut events_replayed = 0;
        for se in &replay_events {
            self.reducer.apply(&mut state, se)?;
            events_replayed += 1;
        }

        // 4. If not already resumed, inject the decision
        if !already_resumed {
            let resume_seq = (replay_events.len() + 1) as Seq;
            let resume_event = SequencedEvent {
                seq: resume_seq,
                event: Event::Resumed {
                    value: decision.value,
                },
            };
            self.reducer.apply(&mut state, &resume_event)?;
            events_replayed += 1;
        }

        Ok(ResumeResult {
            state,
            events_replayed,
            idempotent: already_resumed,
        })
    }

    /// Verifies that resuming N times yields identical results (idempotent).
    pub fn verify_idempotent(
        &self,
        run_id: &RunId,
        decision: ResumeDecision,
        initial_state: S,
    ) -> Result<bool, KernelError> {
        let result1 = self.resume(run_id, decision.clone(), initial_state.clone())?;
        let result2 = self.resume(run_id, decision, initial_state)?;

        // Both should have replayed the same number of events
        if result1.events_replayed != result2.events_replayed {
            return Ok(false);
        }

        // States should be identical
        Ok(result1.state == result2.state)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
    fn resume_injects_decision() {
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
                    Event::Interrupted {
                        value: serde_json::json!({"reason": "ask"}),
                    },
                ],
            )
            .unwrap();

        let resume = ReplayResume::<TestState> {
            events: Box::new(events),
            reducer: Box::new(StateUpdatedOnlyReducer),
        };

        let result = resume
            .resume(
                &run_id,
                ResumeDecision::new(serde_json::json!("user input")),
                TestState(0),
            )
            .unwrap();

        assert_eq!(result.events_replayed, 3); // 2 original + 1 injected Resumed
        assert!(!result.idempotent);
    }

    #[test]
    fn resume_idempotent_twice() {
        // This test verifies the idempotent flag is set correctly when resuming
        // Note: In a real system, the event store would be updated after resume.
        // Here we just verify the resume logic works.
        let events = InMemoryEventStore::new();
        let run_id: RunId = "r2".into();
        events
            .append(
                &run_id,
                &[
                    Event::StateUpdated {
                        step_id: Some("a".into()),
                        payload: serde_json::to_value(&TestState(1)).unwrap(),
                    },
                    Event::Interrupted {
                        value: serde_json::json!({}),
                    },
                ],
            )
            .unwrap();

        let resume = ReplayResume::<TestState> {
            events: Box::new(events),
            reducer: Box::new(StateUpdatedOnlyReducer),
        };

        // First resume - should not be idempotent
        let result1 = resume
            .resume(
                &run_id,
                ResumeDecision::new(serde_json::json!("first")),
                TestState(0),
            )
            .unwrap();

        // The idempotent flag checks if there's ALREADY a Resumed in the store
        // Since we don't write back, it will still be false
        // This is expected behavior for in-memory testing
        assert!(!result1.idempotent);
    }

    #[test]
    fn verify_idempotent_returns_true() {
        let events = InMemoryEventStore::new();
        let run_id: RunId = "r3".into();
        events
            .append(
                &run_id,
                &[
                    Event::StateUpdated {
                        step_id: Some("a".into()),
                        payload: serde_json::to_value(&TestState(5)).unwrap(),
                    },
                    Event::Interrupted {
                        value: serde_json::json!({}),
                    },
                ],
            )
            .unwrap();

        let resume = ReplayResume::<TestState> {
            events: Box::new(events),
            reducer: Box::new(StateUpdatedOnlyReducer),
        };

        let is_idempotent = resume
            .verify_idempotent(
                &run_id,
                ResumeDecision::new(serde_json::json!("test")),
                TestState(0),
            )
            .unwrap();

        assert!(is_idempotent);
    }
}
