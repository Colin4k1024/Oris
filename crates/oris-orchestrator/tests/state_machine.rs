use oris_orchestrator::state::{transition, TaskState, TaskTransitionError};

#[test]
fn release_requires_explicit_approval_path() {
    let state = transition(TaskState::Merged, "request_release").unwrap();
    assert_eq!(state, TaskState::ReleasePendingApproval);

    let err = transition(TaskState::Merged, "publish_without_approval").unwrap_err();
    assert_eq!(err, TaskTransitionError::InvalidTransition);
}
