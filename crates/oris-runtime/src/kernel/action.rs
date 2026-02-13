//! Action and ActionExecutor: single channel for tools and external world (governable).
//!
//! Axiom: tool/LLM calls are system actions; results are recorded only as events (ActionSucceeded/ActionFailed).

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::kernel::identity::RunId;
use crate::kernel::KernelError;

/// System action: the only way the kernel interacts with the outside world.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Action {
    CallTool {
        tool: String,
        input: Value,
    },
    CallLLM {
        provider: String,
        input: Value,
    },
    Sleep {
        millis: u64,
    },
    /// Human-in-the-loop or external signal.
    WaitSignal {
        name: String,
    },
}

/// Result of executing an action (must be turned into events by the driver).
#[derive(Clone, Debug)]
pub enum ActionResult {
    Success(Value),
    Failure(String),
}

/// Executes an action. The driver records ActionRequested, then calls this, then records ActionSucceeded/ActionFailed.
pub trait ActionExecutor: Send + Sync {
    fn execute(&self, run_id: &RunId, action: &Action) -> Result<ActionResult, KernelError>;
}
