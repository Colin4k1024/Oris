//! Run timeline: observable sequence of events for a run (audit, debugging).
//!
//! Built from EventStore; can be serialized to JSON for UI/CLI.

use serde::{Deserialize, Serialize};

use crate::kernel::event::{Event, EventStore};
use crate::kernel::identity::{RunId, Seq};
use crate::kernel::KernelError;

/// One entry in a run timeline (summary of an event at a given seq).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TimelineEntry {
    pub seq: Seq,
    /// Event kind: StateUpdated, ActionRequested, ActionSucceeded, ActionFailed, Interrupted, Resumed, Completed.
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub step_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action_id: Option<String>,
}

/// Full timeline for a run: ordered events and final status.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RunTimeline {
    pub run_id: String,
    pub events: Vec<TimelineEntry>,
    pub final_status: RunStatusSummary,
}

/// Summary of run outcome (for JSON/timeline; mirrors RunStatus).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "status")]
pub enum RunStatusSummary {
    Completed,
    Blocked { interrupt: bool },
    Failed { recoverable: bool },
}

/// Build a RunTimeline from an event store by scanning all events for the run
/// and deriving final status from the last event(s).
pub fn run_timeline(events: &dyn EventStore, run_id: &RunId) -> Result<RunTimeline, KernelError> {
    const FROM_SEQ: Seq = 1;
    let sequenced = events.scan(run_id, FROM_SEQ)?;
    let mut entries = Vec::new();
    let mut final_status = RunStatusSummary::Completed;

    for se in sequenced {
        let (kind, step_id, action_id) = match &se.event {
            Event::StateUpdated { step_id, .. } => {
                ("StateUpdated".to_string(), step_id.clone(), None)
            }
            Event::ActionRequested { action_id, .. } => {
                ("ActionRequested".to_string(), None, Some(action_id.clone()))
            }
            Event::ActionSucceeded { action_id, .. } => {
                ("ActionSucceeded".to_string(), None, Some(action_id.clone()))
            }
            Event::ActionFailed { action_id, .. } => {
                final_status = RunStatusSummary::Failed { recoverable: false };
                ("ActionFailed".to_string(), None, Some(action_id.clone()))
            }
            Event::Interrupted { .. } => {
                final_status = RunStatusSummary::Blocked { interrupt: true };
                ("Interrupted".to_string(), None, None)
            }
            Event::Resumed { .. } => ("Resumed".to_string(), None, None),
            Event::Completed => {
                final_status = RunStatusSummary::Completed;
                ("Completed".to_string(), None, None)
            }
        };
        entries.push(TimelineEntry {
            seq: se.seq,
            kind,
            step_id,
            action_id,
        });
    }

    Ok(RunTimeline {
        run_id: run_id.clone(),
        events: entries,
        final_status,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kernel::event::Event;
    use crate::kernel::event_store::InMemoryEventStore;

    #[test]
    fn timeline_from_events_matches_order_and_final_status() {
        let store = InMemoryEventStore::new();
        let run_id = "timeline-test".to_string();
        store
            .append(
                &run_id,
                &[
                    Event::StateUpdated {
                        step_id: Some("n1".into()),
                        payload: serde_json::json!({"x": 1}),
                    },
                    Event::Completed,
                ],
            )
            .unwrap();
        let tl = run_timeline(&store, &run_id).unwrap();
        assert_eq!(tl.run_id, run_id);
        assert_eq!(tl.events.len(), 2);
        assert_eq!(tl.events[0].seq, 1);
        assert_eq!(tl.events[0].kind, "StateUpdated");
        assert_eq!(tl.events[0].step_id.as_deref(), Some("n1"));
        assert_eq!(tl.events[1].kind, "Completed");
        assert!(matches!(tl.final_status, RunStatusSummary::Completed));
    }

    #[test]
    fn timeline_json_roundtrip() {
        let store = InMemoryEventStore::new();
        let run_id = "json-test".to_string();
        store
            .append(
                &run_id,
                &[
                    Event::ActionRequested {
                        action_id: "a1".into(),
                        payload: serde_json::json!({"tool": "t1"}),
                    },
                    Event::ActionSucceeded {
                        action_id: "a1".into(),
                        output: serde_json::json!("ok"),
                    },
                    Event::Completed,
                ],
            )
            .unwrap();
        let tl = run_timeline(&store, &run_id).unwrap();
        let json = serde_json::to_string(&tl).unwrap();
        let _: RunTimeline = serde_json::from_str(&json).unwrap();
    }
}
