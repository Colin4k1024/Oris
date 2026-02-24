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

/// Classifies executor errors for policy (retry vs fail, backoff, rate-limit).
#[derive(Clone, Debug)]
pub enum ActionErrorKind {
    /// Transient (e.g. network blip); policy may retry.
    Transient,
    /// Permanent (e.g. validation); do not retry.
    Permanent,
    /// Rate-limited (e.g. 429); retry after retry_after_ms if set.
    RateLimited,
}

/// Structured error from action execution; used by Policy for retry decisions.
#[derive(Clone, Debug)]
pub struct ActionError {
    pub kind: ActionErrorKind,
    pub message: String,
    pub retry_after_ms: Option<u64>,
}

impl ActionError {
    pub fn transient(message: impl Into<String>) -> Self {
        Self {
            kind: ActionErrorKind::Transient,
            message: message.into(),
            retry_after_ms: None,
        }
    }

    pub fn permanent(message: impl Into<String>) -> Self {
        Self {
            kind: ActionErrorKind::Permanent,
            message: message.into(),
            retry_after_ms: None,
        }
    }

    pub fn rate_limited(message: impl Into<String>, retry_after_ms: u64) -> Self {
        Self {
            kind: ActionErrorKind::RateLimited,
            message: message.into(),
            retry_after_ms: Some(retry_after_ms),
        }
    }

    /// Convert a generic executor error (KernelError) into an ActionError for policy.
    /// Used by the driver when the executor returns Err(KernelError).
    pub fn from_kernel_error(e: &KernelError) -> Self {
        if let KernelError::Executor(ae) = e {
            ae.clone()
        } else {
            Self::permanent(e.to_string())
        }
    }
}

impl std::fmt::Display for ActionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

/// Executes an action. The driver records ActionRequested, then calls this, then records ActionSucceeded/ActionFailed.
/// Return `Err(KernelError::Executor(ActionError))` for structured retry decisions; other `KernelError` are treated as permanent.
pub trait ActionExecutor: Send + Sync {
    fn execute(&self, run_id: &RunId, action: &Action) -> Result<ActionResult, KernelError>;
}
