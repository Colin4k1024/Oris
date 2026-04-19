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
    /// Invoke a named tool with a JSON payload.
    CallTool {
        /// Name of the tool to invoke.
        tool: String,
        /// JSON input passed to the tool.
        input: Value,
    },
    /// Invoke an LLM provider with a JSON input.
    CallLLM {
        /// Identifier of the LLM provider (e.g. `"openai"`).
        provider: String,
        /// JSON input (prompt, messages, params).
        input: Value,
    },
    /// Suspend execution for a fixed duration.
    Sleep {
        /// Duration in milliseconds.
        millis: u64,
    },
    /// Human-in-the-loop or external signal.
    WaitSignal {
        /// Name of the expected signal channel.
        name: String,
    },
}

/// Result of executing an action (must be turned into events by the driver).
#[derive(Clone, Debug)]
pub enum ActionResult {
    /// The action completed; the value is recorded as the action output.
    Success(Value),
    /// The action failed; the string is the error message recorded in the event log.
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
    /// Whether the failure is transient, permanent, or rate-limited.
    pub kind: ActionErrorKind,
    /// Human-readable error message for logging and auditing.
    pub message: String,
    /// For rate-limited errors: suggested retry delay in milliseconds.
    pub retry_after_ms: Option<u64>,
}

impl ActionError {
    /// Creates a transient error (network blip, timeout). The policy may retry.
    pub fn transient(message: impl Into<String>) -> Self {
        Self {
            kind: ActionErrorKind::Transient,
            message: message.into(),
            retry_after_ms: None,
        }
    }

    /// Creates a permanent error (bad input, not-found). The policy must not retry.
    pub fn permanent(message: impl Into<String>) -> Self {
        Self {
            kind: ActionErrorKind::Permanent,
            message: message.into(),
            retry_after_ms: None,
        }
    }

    /// Creates a rate-limited error with the suggested backoff delay in milliseconds.
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
    /// Performs the given action for `run_id` and returns its result.
    ///
    /// Return `Ok(ActionResult::Failure(_))` for logical failures the policy should not retry.
    /// Return `Err(KernelError::Executor(_))` to let the policy decide on retry.
    fn execute(&self, run_id: &RunId, action: &Action) -> Result<ActionResult, KernelError>;
}
