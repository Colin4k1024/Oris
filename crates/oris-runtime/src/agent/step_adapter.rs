//! AgentStepFnAdapter: wraps UnifiedAgent as a sync StepFn for the kernel.
//!
//! Runs one agent "step" via block_on. Currently one step = full invoke; a future
//! extension can map one plan + optional tool call to Next::Do(CallTool) when the
//! executor exposes single-step execution.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::kernel::event::Event;
use crate::kernel::state::KernelState;
use crate::kernel::step::{Next, StepFn};
use crate::kernel::KernelError;
use crate::prompt::PromptArgs;

use super::unified_agent::{AgentInput, UnifiedAgent};
use crate::graph::RunnableConfig;

/// State for the kernel when driving an agent: prompt args (input) and optional last output.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct AgentStepState {
    /// Input for the agent (e.g. chat_history, input).
    #[serde(default)]
    pub prompt_args: PromptArgs,
    /// Last output from the agent (set by reducer on StateUpdated).
    #[serde(default)]
    pub last_output: Option<String>,
}

impl AgentStepState {
    pub fn new(prompt_args: PromptArgs) -> Self {
        Self {
            prompt_args,
            last_output: None,
        }
    }
}

impl KernelState for AgentStepState {
    fn version(&self) -> u32 {
        1
    }
}

/// Sync StepFn that runs the agent (one full invoke per step) via block_on.
pub struct AgentStepFnAdapter {
    pub agent: Arc<UnifiedAgent>,
    pub config: RunnableConfig,
}

impl AgentStepFnAdapter {
    pub fn new(agent: Arc<UnifiedAgent>, config: RunnableConfig) -> Self {
        Self { agent, config }
    }
}

impl StepFn<AgentStepState> for AgentStepFnAdapter {
    /// Callers must invoke the kernel from a thread that has an entered Tokio runtime;
    /// do not call from inside an async task (to avoid blocking the runtime).
    fn next(&self, state: &AgentStepState) -> Result<Next, KernelError> {
        let agent = Arc::clone(&self.agent);
        let config = self.config.clone();
        let prompt_args = state.prompt_args.clone();
        let result = tokio::runtime::Handle::current().block_on(async move {
            agent
                .invoke_with_config(AgentInput::State(prompt_args), &config)
                .await
        });
        match result {
            Ok(super::AgentInvokeResult::Complete(output)) => {
                let payload = serde_json::json!({ "output": output });
                Ok(Next::Emit(vec![Event::StateUpdated {
                    step_id: Some("agent".to_string()),
                    payload,
                }]))
            }
            Ok(super::AgentInvokeResult::Interrupt { interrupt_value }) => {
                Ok(Next::Interrupt(crate::kernel::step::InterruptInfo {
                    value: interrupt_value,
                }))
            }
            Err(e) => Err(KernelError::Driver(e.to_string())),
        }
    }
}
