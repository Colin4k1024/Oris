//! Kernel driver: run_until_blocked, resume, replay.

use std::time::Duration;

use crate::kernel::action::{ActionExecutor, ActionResult};
use crate::kernel::event::{Event, EventStore};
use crate::kernel::identity::{RunId, Seq};
use crate::kernel::policy::{Policy, PolicyCtx, RetryDecision};
use crate::kernel::reducer::Reducer;
use crate::kernel::snapshot::{Snapshot, SnapshotStore};
use crate::kernel::state::KernelState;
use crate::kernel::step::{InterruptInfo, Next, StepFn};
use crate::kernel::KernelError;

/// Standardized status of a run after run_until_blocked or resume.
#[derive(Clone, Debug)]
pub enum RunStatus {
    /// Run completed successfully.
    Completed,
    /// Run is blocked (e.g. on interrupt or WaitSignal); can be resumed.
    Blocked(BlockedInfo),
    /// Run is still advancing (optional; used when yielding before blocking).
    Running,
    /// Run failed; recoverable indicates whether resume/retry is possible.
    Failed { recoverable: bool },
}

#[derive(Clone, Debug)]
pub struct BlockedInfo {
    pub interrupt: Option<InterruptInfo>,
    pub wait_signal: Option<String>,
}

/// Signal to resume a blocked run (e.g. human approval, external event).
#[derive(Clone, Debug)]
pub enum Signal {
    Resume(serde_json::Value),
    Signal { name: String, value: serde_json::Value },
}

/// Kernel: event store, optional snapshot store, reducer, executor, step fn, policy.
pub struct Kernel<S: KernelState> {
    pub events: Box<dyn EventStore>,
    pub snaps: Option<Box<dyn SnapshotStore<S>>>,
    pub reducer: Box<dyn Reducer<S>>,
    pub exec: Box<dyn ActionExecutor>,
    pub step: Box<dyn StepFn<S>>,
    pub policy: Box<dyn Policy>,
}

impl<S: KernelState> Kernel<S> {
    /// Runs until the step returns Complete or Interrupt/WaitSignal. Returns run status.
    /// State is obtained by replaying the event log (or initial_state if the run has no events yet).
    pub fn run_until_blocked(
        &self,
        run_id: &RunId,
        initial_state: S,
    ) -> Result<RunStatus, KernelError> {
        self.run_loop(run_id, initial_state)
    }

    /// Resumes a blocked run with a signal (e.g. resume value or external signal).
    /// Appends Resumed (or Signal) event, then runs until blocked or complete.
    pub fn resume(
        &self,
        run_id: &RunId,
        initial_state: S,
        signal: Signal,
    ) -> Result<RunStatus, KernelError> {
        let value = match &signal {
            Signal::Resume(v) => v.clone(),
            Signal::Signal { value, .. } => value.clone(),
        };
        self.events
            .append(run_id, &[Event::Resumed { value }])?;
        self.run_loop(run_id, initial_state)
    }

    /// Inner loop: replay to get state, then step until Complete or Blocked.
    fn run_loop(&self, run_id: &RunId, initial_state: S) -> Result<RunStatus, KernelError> {
        const FROM_SEQ: Seq = 1;
        let mut state = initial_state;
        let sequenced = self.events.scan(run_id, FROM_SEQ)?;
        for se in sequenced {
            self.reducer.apply(&mut state, &se)?;
        }

        loop {
            let next = self.step.next(&state)?;
            match next {
                Next::Emit(evs) => {
                    if !evs.is_empty() {
                        let before = self.events.head(run_id)?;
                        self.events.append(run_id, &evs)?;
                        let new_events = self.events.scan(run_id, before + 1)?;
                        for se in new_events {
                            self.reducer.apply(&mut state, &se)?;
                        }
                    }
                }
                Next::Do(action) => {
                    self.policy.authorize(run_id, &action, &PolicyCtx::default())?;
                    let before = self.events.head(run_id)?;
                    let action_id = format!("{}-{}", run_id, before + 1);
                    let payload = serde_json::to_value(&action)
                        .map_err(|e| KernelError::Driver(e.to_string()))?;
                    self.events.append(
                        run_id,
                        &[Event::ActionRequested {
                            action_id: action_id.clone(),
                            payload,
                        }],
                    )?;
                    let result = self.exec.execute(run_id, &action);
                    match result {
                        Ok(ActionResult::Success(output)) => {
                            self.events.append(
                                run_id,
                                &[Event::ActionSucceeded {
                                    action_id: action_id.clone(),
                                    output,
                                }],
                            )?;
                        }
                        Ok(ActionResult::Failure(error)) => {
                            self.events.append(
                                run_id,
                                &[Event::ActionFailed {
                                    action_id,
                                    error,
                                }],
                            )?;
                        }
                        Err(mut e) => {
                            let mut attempt = 0u32;
                            loop {
                                let decision =
                                    self.policy.retry_strategy_attempt(&e, &action, attempt);
                                match decision {
                                    RetryDecision::Fail => {
                                        self.events.append(
                                            run_id,
                                            &[Event::ActionFailed {
                                                action_id: action_id.clone(),
                                                error: e.to_string(),
                                            }],
                                        )?;
                                        return Ok(RunStatus::Failed { recoverable: false });
                                    }
                                    RetryDecision::Retry | RetryDecision::RetryAfterMs(0) => {}
                                    RetryDecision::RetryAfterMs(ms) => {
                                        std::thread::sleep(Duration::from_millis(ms));
                                    }
                                }
                                attempt += 1;
                                match self.exec.execute(run_id, &action) {
                                    Ok(ActionResult::Success(output)) => {
                                        self.events.append(
                                            run_id,
                                            &[Event::ActionSucceeded {
                                                action_id: action_id.clone(),
                                                output,
                                            }],
                                        )?;
                                        break;
                                    }
                                    Ok(ActionResult::Failure(error)) => {
                                        self.events.append(
                                            run_id,
                                            &[Event::ActionFailed {
                                                action_id: action_id.clone(),
                                                error,
                                            }],
                                        )?;
                                        break;
                                    }
                                    Err(e2) => e = e2,
                                }
                            }
                        }
                    }
                    let new_events = self.events.scan(run_id, before + 1)?;
                    for se in new_events {
                        self.reducer.apply(&mut state, &se)?;
                    }
                }
                Next::Interrupt(info) => {
                    self.events.append(
                        run_id,
                        &[Event::Interrupted {
                            value: info.value.clone(),
                        }],
                    )?;
                    return Ok(RunStatus::Blocked(BlockedInfo {
                        interrupt: Some(info),
                        wait_signal: None,
                    }));
                }
                Next::Complete => {
                    self.events.append(run_id, &[Event::Completed])?;
                    return Ok(RunStatus::Completed);
                }
            }
        }
    }

    /// Replays the run from the event log without executing external actions; returns final state.
    ///
    /// Scans all events for the run (from seq 1), applies each with the Reducer in order.
    /// Does not call ActionExecutor; any ActionRequested is satisfied by the following
    /// ActionSucceeded/ActionFailed already stored in the log (reducer applies them).
    pub fn replay(&self, run_id: &RunId, initial_state: S) -> Result<S, KernelError> {
        self.replay_from(run_id, initial_state, None)
    }

    /// Replays from a snapshot if available, otherwise from initial state.
    ///
    /// If `snaps` is set and `load_latest(run_id)` returns a snapshot, state starts at
    /// `snap.state` and only events with seq > snap.at_seq are applied; otherwise
    /// starts at `initial_state` and replays from seq 1.
    pub fn replay_from_snapshot(
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

    fn replay_from(
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kernel::action::{Action, ActionExecutor, ActionResult};
    use crate::kernel::event::Event;
    use crate::kernel::event_store::{InMemoryEventStore, SharedEventStore};
    use crate::kernel::stubs::{AllowAllPolicy, NoopActionExecutor, NoopStepFn};
    use crate::kernel::StateUpdatedOnlyReducer;
    use serde::{Deserialize, Serialize};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    #[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
    struct TestState(u32);
    impl KernelState for TestState {
        fn version(&self) -> u32 {
            1
        }
    }

    /// Mock executor that counts execute() calls; used to assert replay does not call executor.
    struct CountingActionExecutor(Arc<AtomicUsize>);
    impl CountingActionExecutor {
        fn new(counter: Arc<AtomicUsize>) -> Self {
            Self(counter)
        }
        fn count(&self) -> usize {
            self.0.load(Ordering::SeqCst)
        }
    }
    impl ActionExecutor for CountingActionExecutor {
        fn execute(&self, _run_id: &RunId, _action: &Action) -> Result<ActionResult, KernelError> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Err(KernelError::Driver("mock".into()))
        }
    }

    #[test]
    fn run_until_blocked_complete() {
        let k = Kernel::<TestState> {
            events: Box::new(InMemoryEventStore::new()),
            snaps: None,
            reducer: Box::new(StateUpdatedOnlyReducer),
            exec: Box::new(NoopActionExecutor),
            step: Box::new(NoopStepFn),
            policy: Box::new(AllowAllPolicy),
        };
        let run_id = "run-complete".to_string();
        let status = k.run_until_blocked(&run_id, TestState(0)).unwrap();
        assert!(matches!(status, RunStatus::Completed));
    }

    struct InterruptOnceStep(bool);
    impl StepFn<TestState> for InterruptOnceStep {
        fn next(&self, _state: &TestState) -> Result<Next, KernelError> {
            if !self.0 {
                Ok(Next::Interrupt(InterruptInfo {
                    value: serde_json::json!("pause"),
                }))
            } else {
                Ok(Next::Complete)
            }
        }
    }

    #[test]
    fn run_until_blocked_then_resume() {
        let inner = Arc::new(InMemoryEventStore::new());
        let events = Box::new(SharedEventStore(inner.clone()));
        let run_id = "run-interrupt-resume".to_string();
        let k = Kernel::<TestState> {
            events: events,
            snaps: None,
            reducer: Box::new(StateUpdatedOnlyReducer),
            exec: Box::new(NoopActionExecutor),
            step: Box::new(InterruptOnceStep(false)),
            policy: Box::new(AllowAllPolicy),
        };
        let status = k.run_until_blocked(&run_id, TestState(0)).unwrap();
        assert!(matches!(status, RunStatus::Blocked(_)));

        let k2 = Kernel::<TestState> {
            events: Box::new(SharedEventStore(inner)),
            snaps: None,
            reducer: Box::new(StateUpdatedOnlyReducer),
            exec: Box::new(NoopActionExecutor),
            step: Box::new(InterruptOnceStep(true)),
            policy: Box::new(AllowAllPolicy),
        };
        let status2 = k2.resume(&run_id, TestState(0), Signal::Resume(serde_json::json!(1))).unwrap();
        assert!(matches!(status2, RunStatus::Completed));
    }

    /// Replay must not call ActionExecutor (0 side effects).
    #[test]
    fn replay_no_side_effects() {
        let exec_count = Arc::new(AtomicUsize::new(0));
        let store = InMemoryEventStore::new();
        let run_id = "replay-no-effects".to_string();
        store
            .append(
                &run_id,
                &[
                    Event::StateUpdated {
                        step_id: Some("n1".into()),
                        payload: serde_json::json!(42),
                    },
                    Event::Completed,
                ],
            )
            .unwrap();
        let k = Kernel::<TestState> {
            events: Box::new(store),
            snaps: None,
            reducer: Box::new(StateUpdatedOnlyReducer),
            exec: Box::new(CountingActionExecutor::new(Arc::clone(&exec_count))),
            step: Box::new(NoopStepFn),
            policy: Box::new(AllowAllPolicy),
        };
        let _ = k.replay(&run_id, TestState(0)).unwrap();
        assert_eq!(exec_count.load(Ordering::SeqCst), 0, "replay must not call executor");
    }

    /// Same event log and initial state must yield identical state on multiple replays.
    #[test]
    fn replay_state_equivalence() {
        let store = InMemoryEventStore::new();
        let run_id = "replay-equiv".to_string();
        store
            .append(
                &run_id,
                &[
                    Event::StateUpdated {
                        step_id: Some("a".into()),
                        payload: serde_json::json!(10),
                    },
                    Event::StateUpdated {
                        step_id: Some("b".into()),
                        payload: serde_json::json!(20),
                    },
                    Event::Completed,
                ],
            )
            .unwrap();
        let k = Kernel::<TestState> {
            events: Box::new(SharedEventStore(Arc::new(store))),
            snaps: None,
            reducer: Box::new(StateUpdatedOnlyReducer),
            exec: Box::new(NoopActionExecutor),
            step: Box::new(NoopStepFn),
            policy: Box::new(AllowAllPolicy),
        };
        let initial = TestState(0);
        let s1 = k.replay(&run_id, initial.clone()).unwrap();
        let s2 = k.replay(&run_id, initial).unwrap();
        assert_eq!(s1, s2, "same log and initial state must yield equal state");
        assert_eq!(s1.0, 20);
    }

    /// Failure path: executor returns Err → RunStatus::Failed and event log has ActionFailed for that action_id.
    #[test]
    fn run_until_blocked_failure_recovery() {
        let store = Arc::new(InMemoryEventStore::new());
        let run_id = "run-fail".to_string();
        let k = Kernel::<TestState> {
            events: Box::new(SharedEventStore(Arc::clone(&store))),
            snaps: None,
            reducer: Box::new(StateUpdatedOnlyReducer),
            exec: Box::new(CountingActionExecutor::new(Arc::new(AtomicUsize::new(0)))),
            step: Box::new(DoOnceStep),
            policy: Box::new(AllowAllPolicy),
        };
        let status = k.run_until_blocked(&run_id, TestState(0)).unwrap();
        assert!(
            matches!(status, RunStatus::Failed { recoverable } if !recoverable),
            "default policy says Fail so recoverable should be false"
        );
        let events = store.scan(&run_id, 1).unwrap();
        let has_requested = events.iter().any(|e| matches!(e.event, Event::ActionRequested { .. }));
        let has_failed = events.iter().any(|e| matches!(e.event, Event::ActionFailed { .. }));
        assert!(has_requested && has_failed, "event log must contain ActionRequested and ActionFailed for the same action");
    }

    /// Step that returns Next::Do once so the driver executes one action.
    struct DoOnceStep;
    impl StepFn<TestState> for DoOnceStep {
        fn next(&self, _state: &TestState) -> Result<Next, KernelError> {
            Ok(Next::Do(Action::CallTool {
                tool: "dummy".into(),
                input: serde_json::json!(null),
            }))
        }
    }

    /// Replay from snapshot applies only events after at_seq.
    #[test]
    fn replay_from_snapshot_applies_tail_only() {
        use crate::kernel::InMemorySnapshotStore;
        use crate::kernel::Snapshot;

        let store = InMemoryEventStore::new();
        let run_id = "replay-snap".to_string();
        store
            .append(
                &run_id,
                &[
                    Event::StateUpdated {
                        step_id: Some("a".into()),
                        payload: serde_json::json!(10),
                    },
                    Event::StateUpdated {
                        step_id: Some("b".into()),
                        payload: serde_json::json!(20),
                    },
                    Event::StateUpdated {
                        step_id: Some("c".into()),
                        payload: serde_json::json!(30),
                    },
                    Event::Completed,
                ],
            )
            .unwrap();
        let snaps = InMemorySnapshotStore::new();
        snaps
            .save(&Snapshot {
                run_id: run_id.clone(),
                at_seq: 2,
                state: TestState(20),
            })
            .unwrap();
        let k = Kernel::<TestState> {
            events: Box::new(store),
            snaps: Some(Box::new(snaps)),
            reducer: Box::new(StateUpdatedOnlyReducer),
            exec: Box::new(NoopActionExecutor),
            step: Box::new(NoopStepFn),
            policy: Box::new(AllowAllPolicy),
        };
        let state = k
            .replay_from_snapshot(&run_id, TestState(0))
            .unwrap();
        assert_eq!(state.0, 30, "only events after at_seq=2 (seq 3) applied");
    }
}
