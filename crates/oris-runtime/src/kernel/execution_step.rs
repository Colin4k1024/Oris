//! Execution step contract: canonical interface for one step of execution.
//!
//! This module defines the **frozen** execution contract so that adapters (graph, agent, etc.)
//! interact with the kernel only through explicit inputs and outputs. No hidden runtime
//! mutations or async side effects inside the boundary; all effects are captured in
//! [StepResult].

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::kernel::state::KernelState;
use crate::kernel::step::Next;
use crate::kernel::KernelError;

/// Explicit input for one execution step.
///
/// All inputs to a step are declared here so the boundary is pure and replayable.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ExecutionStepInput {
    /// Initial step (no prior resume or signal).
    Initial,
    /// Resuming after an interrupt; payload is the resolved value.
    Resume(Value),
    /// External signal (e.g. named channel).
    Signal { name: String, value: Value },
}

impl ExecutionStepInput {
    /// Validates that the input is well-formed (e.g. Resume has a value).
    pub fn validate(&self) -> Result<(), KernelError> {
        match self {
            ExecutionStepInput::Initial => Ok(()),
            ExecutionStepInput::Resume(_) => Ok(()),
            ExecutionStepInput::Signal { name, .. } => {
                if name.is_empty() {
                    return Err(KernelError::Driver(
                        "ExecutionStepInput::Signal name must be non-empty".into(),
                    ));
                }
                Ok(())
            }
        }
    }
}

/// Result of one execution step: the only way a step can affect the run.
///
/// This is the canonical output type of the execution contract. Adapters must not
/// mutate state or trigger side effects except by returning a [StepResult] that
/// the driver then applies (emit events, do action, interrupt, complete).
pub type StepResult = Next;

/// Canonical execution step trait: pure boundary between kernel and adapters.
///
/// Implementations must:
/// - Take only `state` and `input` as inputs (no hidden mutable state, no I/O).
/// - Return only a [StepResult] (or error); all effects go through the result.
///
/// The driver calls `execute(state, input)` and applies the returned [StepResult];
/// adapters (e.g. graph, agent) implement this trait and interact solely through it.
pub trait ExecutionStep<S: KernelState>: Send + Sync {
    /// Performs one step: given current state and explicit input, returns the next outcome.
    ///
    /// Pure boundary: no async, no hidden mutations. Inputs and outputs are explicit.
    fn execute(&self, state: &S, input: &ExecutionStepInput) -> Result<StepResult, KernelError>;
}

/// Blanket impl: any [crate::kernel::step::StepFn] is an [ExecutionStep] with input ignored.
///
/// This keeps existing step functions (e.g. [crate::graph::step_adapter::GraphStepFnAdapter])
/// valid under the frozen contract; they receive [ExecutionStepInput::Initial] each time.
impl<S, F> ExecutionStep<S> for F
where
    S: KernelState,
    F: crate::kernel::step::StepFn<S>,
{
    fn execute(&self, state: &S, _input: &ExecutionStepInput) -> Result<StepResult, KernelError> {
        self.next(state)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kernel::event::Event;
    use crate::kernel::state::KernelState;
    use crate::kernel::step::StepFn;

    #[derive(Clone, Debug)]
    struct TestState(u32);
    impl KernelState for TestState {
        fn version(&self) -> u32 {
            1
        }
    }

    #[test]
    fn execution_step_input_initial_validates() {
        let input = ExecutionStepInput::Initial;
        assert!(input.validate().is_ok());
    }

    #[test]
    fn execution_step_input_resume_validates() {
        let input = ExecutionStepInput::Resume(serde_json::json!({"ok": true}));
        assert!(input.validate().is_ok());
    }

    #[test]
    fn execution_step_input_signal_empty_name_invalid() {
        let input = ExecutionStepInput::Signal {
            name: String::new(),
            value: serde_json::json!(null),
        };
        assert!(input.validate().is_err());
    }

    #[test]
    fn execution_step_input_signal_non_empty_name_validates() {
        let input = ExecutionStepInput::Signal {
            name: "evt".to_string(),
            value: serde_json::json!(1),
        };
        assert!(input.validate().is_ok());
    }

    struct StepThatEmits;
    impl StepFn<TestState> for StepThatEmits {
        fn next(&self, state: &TestState) -> Result<Next, KernelError> {
            Ok(Next::Emit(vec![Event::StateUpdated {
                step_id: None,
                payload: serde_json::json!({ "n": state.0 }),
            }]))
        }
    }

    #[test]
    fn step_fn_impls_execution_step() {
        let step = StepThatEmits;
        let state = TestState(42);
        let input = ExecutionStepInput::Initial;
        let result = step.execute(&state, &input).unwrap();
        match result {
            Next::Emit(evs) => {
                assert_eq!(evs.len(), 1);
            }
            _ => panic!("expected Emit"),
        }
    }
}
