//! Event type and EventStore for the Oris kernel.
//!
//! Events are the source of truth. All state is derived by reducing events.
//! Constraints: append is atomic (all or nothing); every event has a seq; scan returns ordered by seq.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::kernel::identity::{RunId, Seq};

/// A single event in the kernel event log.
///
/// Covers: state updates, action lifecycle, interrupt/resume, completion.
/// Aligns with existing trace (StepCompleted → StateUpdated + optional Action*; InterruptReached → Interrupted; ResumeReceived → Resumed).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Event {
    /// State was updated by the reducer (e.g. after a node step).
    StateUpdated {
        /// Optional step/node identifier.
        step_id: Option<String>,
        /// Serialized state or state delta (schema depends on State type).
        payload: Value,
    },
    /// An external action was requested (tool, LLM, sleep, wait signal).
    ActionRequested {
        /// Unique id for this action instance (for matching with result).
        action_id: String,
        /// Kind and input (e.g. CallTool { tool, input }).
        payload: Value,
    },
    /// The action completed successfully; output is stored for replay.
    ActionSucceeded { action_id: String, output: Value },
    /// The action failed; error is stored for audit and retry policy.
    ActionFailed { action_id: String, error: String },
    /// Execution was interrupted (e.g. human-in-the-loop).
    Interrupted { value: Value },
    /// Execution was resumed with a value after an interrupt.
    Resumed { value: Value },
    /// The run completed.
    Completed,
}

/// An event with its assigned sequence number (store may assign seq on append).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SequencedEvent {
    pub seq: Seq,
    pub event: Event,
}

/// Event store: append-only log per run, source of truth.
///
/// **Constraints (must hold in all implementations and tests):**
/// - `append`: either all events in the batch succeed or none (atomicity).
/// - Each event has a seq (assigned by store or caller).
/// - `scan(run_id, from)` returns events in **ascending seq order**.
pub trait EventStore: Send + Sync {
    /// Appends events for the given run. Returns the seq of the last written event (or an error).
    /// Implementations must assign seqs if not present and guarantee atomicity.
    fn append(&self, run_id: &RunId, events: &[Event]) -> Result<Seq, KernelError>;

    /// Scans events for the run starting at `from` (inclusive), in ascending seq order.
    fn scan(&self, run_id: &RunId, from: Seq) -> Result<Vec<SequencedEvent>, KernelError>;

    /// Returns the highest seq for the run (0 if no events).
    fn head(&self, run_id: &RunId) -> Result<Seq, KernelError>;
}

/// Kernel-level error type.
#[derive(Debug, thiserror::Error)]
pub enum KernelError {
    #[error("EventStore error: {0}")]
    EventStore(String),
    #[error("SnapshotStore error: {0}")]
    SnapshotStore(String),
    #[error("Reducer error: {0}")]
    Reducer(String),
    #[error("Policy error: {0}")]
    Policy(String),
    #[error("Driver error: {0}")]
    Driver(String),
    /// Executor returned a structured action error (for policy retry decisions).
    #[error("Executor: {0}")]
    Executor(crate::kernel::action::ActionError),
}
