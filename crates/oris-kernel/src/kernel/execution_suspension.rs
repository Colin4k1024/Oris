//! Execution suspension state machine: handle worker teardown when execution is paused.
//!
//! This module defines [ExecutionSuspensionState] for tracking execution lifecycle:
//! - `Running`: normal execution.
//! - `Suspended`: execution paused, worker preparing to exit.
//! - `WaitingInput`: waiting for external input (human, tool, policy).

use serde::{Deserialize, Serialize};

use crate::kernel::identity::RunId;

/// Execution suspension state.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExecutionSuspensionState {
    /// Normal execution.
    #[default]
    Running,
    /// Execution paused, worker preparing to exit and release resources.
    Suspended,
    /// Waiting for external input (human, tool, policy).
    WaitingInput,
}

/// Execution suspension: tracks the state of execution suspension.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExecutionSuspension {
    /// Run this suspension belongs to.
    pub run_id: RunId,
    /// Current state.
    pub state: ExecutionSuspensionState,
    /// When the state last changed.
    pub state_changed_at: chrono::DateTime<chrono::Utc>,
    /// Optional reason for the suspension.
    pub reason: Option<String>,
}

impl ExecutionSuspension {
    /// Creates a new execution suspension in Running state.
    pub fn new(run_id: RunId) -> Self {
        Self {
            run_id,
            state: ExecutionSuspensionState::Running,
            state_changed_at: chrono::Utc::now(),
            reason: None,
        }
    }

    /// Transition from Running to Suspended.
    pub fn suspend(&mut self, reason: Option<String>) -> Result<(), SuspensionError> {
        if self.state != ExecutionSuspensionState::Running {
            return Err(SuspensionError::InvalidTransition {
                from: self.state.clone(),
                to: "Suspended".into(),
            });
        }
        self.state = ExecutionSuspensionState::Suspended;
        self.state_changed_at = chrono::Utc::now();
        self.reason = reason;
        Ok(())
    }

    /// Transition from Suspended to WaitingInput.
    pub fn wait_input(&mut self) -> Result<(), SuspensionError> {
        if self.state != ExecutionSuspensionState::Suspended {
            return Err(SuspensionError::InvalidTransition {
                from: self.state.clone(),
                to: "WaitingInput".into(),
            });
        }
        self.state = ExecutionSuspensionState::WaitingInput;
        self.state_changed_at = chrono::Utc::now();
        Ok(())
    }

    /// Transition from WaitingInput to Running (resume).
    pub fn resume(&mut self) -> Result<(), SuspensionError> {
        if self.state != ExecutionSuspensionState::WaitingInput {
            return Err(SuspensionError::InvalidTransition {
                from: self.state.clone(),
                to: "Running".into(),
            });
        }
        self.state = ExecutionSuspensionState::Running;
        self.state_changed_at = chrono::Utc::now();
        self.reason = None;
        Ok(())
    }

    /// Check if currently in Running state.
    pub fn is_running(&self) -> bool {
        self.state == ExecutionSuspensionState::Running
    }

    /// Check if currently suspended or waiting for input.
    pub fn is_suspended(&self) -> bool {
        matches!(
            self.state,
            ExecutionSuspensionState::Suspended | ExecutionSuspensionState::WaitingInput
        )
    }
}

/// Errors for suspension operations.
#[derive(Debug, thiserror::Error)]
pub enum SuspensionError {
    #[error("Invalid state transition from {from:?} to {to}")]
    InvalidTransition {
        from: ExecutionSuspensionState,
        to: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_suspension_is_running() {
        let susp = ExecutionSuspension::new("run-1".into());
        assert!(susp.is_running());
        assert!(!susp.is_suspended());
    }

    #[test]
    fn running_to_suspended_transition() {
        let mut susp = ExecutionSuspension::new("run-1".into());
        susp.suspend(Some("user requested".into())).unwrap();
        assert!(!susp.is_running());
        assert!(susp.is_suspended());
        assert_eq!(susp.state, ExecutionSuspensionState::Suspended);
    }

    #[test]
    fn suspended_to_waiting_input() {
        let mut susp = ExecutionSuspension::new("run-1".into());
        susp.suspend(None).unwrap();
        susp.wait_input().unwrap();
        assert_eq!(susp.state, ExecutionSuspensionState::WaitingInput);
    }

    #[test]
    fn waiting_input_to_running_resume() {
        let mut susp = ExecutionSuspension::new("run-1".into());
        susp.suspend(None).unwrap();
        susp.wait_input().unwrap();
        susp.resume().unwrap();
        assert!(susp.is_running());
    }

    #[test]
    fn invalid_transition_running_to_waiting() {
        let mut susp = ExecutionSuspension::new("run-1".into());
        let err = susp.wait_input().unwrap_err();
        println!("Error: {:?}", err);
        assert!(err.to_string().contains("Invalid state transition"));
    }
}
