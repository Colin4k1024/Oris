#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskState {
    Queued,
    Planned,
    Dispatched,
    InProgress,
    Validated,
    PRReady,
    Merged,
    ReleasePendingApproval,
    Released,
    FailedRetryable,
    FailedTerminal,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskTransitionError {
    InvalidTransition,
}

pub fn transition(state: TaskState, event: &str) -> Result<TaskState, TaskTransitionError> {
    match (state, event) {
        (TaskState::Merged, "request_release") => Ok(TaskState::ReleasePendingApproval),
        (TaskState::ReleasePendingApproval, "approve_release") => Ok(TaskState::Released),
        _ => Err(TaskTransitionError::InvalidTransition),
    }
}
