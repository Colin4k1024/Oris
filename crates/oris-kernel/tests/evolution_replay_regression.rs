use std::sync::Arc;

use oris_kernel::{
    event_stream_hash, Event, EventStore, InMemoryEventStore, KernelState, ReplayCursor, RunId,
    SharedEventStore, StateUpdatedOnlyReducer,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct ReplayState {
    counter: u32,
    label: String,
}

impl KernelState for ReplayState {
    fn version(&self) -> u32 {
        1
    }
}

#[test]
fn repeated_replay_preserves_state_and_event_hash() {
    let events = Arc::new(InMemoryEventStore::new());
    let writer = SharedEventStore(events.clone());
    let run_id: RunId = "evolution-replay-regression".into();
    let initial_state = ReplayState {
        counter: 0,
        label: "start".into(),
    };
    let expected_state = ReplayState {
        counter: 2,
        label: "validated".into(),
    };

    writer
        .append(
            &run_id,
            &[
                Event::StateUpdated {
                    step_id: Some("prepare".into()),
                    payload: serde_json::to_value(ReplayState {
                        counter: 1,
                        label: "captured".into(),
                    })
                    .unwrap(),
                },
                Event::StateUpdated {
                    step_id: Some("replay".into()),
                    payload: serde_json::to_value(expected_state.clone()).unwrap(),
                },
                Event::Completed,
            ],
        )
        .unwrap();

    let hash_before = event_stream_hash(&writer, &run_id).unwrap();

    let cursor = ReplayCursor::<ReplayState> {
        events: Box::new(SharedEventStore(events.clone())),
        snaps: None,
        reducer: Box::new(StateUpdatedOnlyReducer),
    };

    let first = cursor.replay(&run_id, initial_state.clone()).unwrap();
    let second = cursor.replay(&run_id, initial_state).unwrap();
    let hash_after = event_stream_hash(&SharedEventStore(events), &run_id).unwrap();

    assert_eq!(first, expected_state);
    assert_eq!(second, expected_state);
    assert_eq!(
        first, second,
        "identical logs must replay to the same state"
    );
    assert_eq!(
        hash_before, hash_after,
        "replaying must not mutate the recorded event stream"
    );
}
