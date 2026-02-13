//! Policy: governance layer (authorize, retry, budget).
//!
//! Must exist even as a minimal implementation so Oris is not a "run any tool" demo.
//!
//! **Retry loop:** The driver calls `retry_strategy_attempt` on executor `Err` and only stops when
//! the policy returns `Fail`. Implementations must eventually return `Fail` or the loop would not
//! terminate; `RetryWithBackoffPolicy` does so after `max_retries` attempts.

use std::collections::HashSet;

use crate::kernel::action::Action;
use crate::kernel::identity::RunId;
use crate::kernel::KernelError;

/// Context passed to policy (e.g. caller identity, run metadata).
#[derive(Clone, Debug, Default)]
pub struct PolicyCtx {
    pub user_id: Option<String>,
    pub metadata: std::collections::HashMap<String, String>,
}

/// Decision after an action failure (retry, backoff, or fail).
#[derive(Clone, Debug)]
pub enum RetryDecision {
    Retry,
    RetryAfterMs(u64),
    Fail,
}

/// Optional budget rules (cost, token limits, etc.).
#[derive(Clone, Debug, Default)]
pub struct BudgetRules {
    pub max_tool_calls: Option<u64>,
    pub max_llm_tokens: Option<u64>,
}

/// Policy: authorize actions, decide retries, optional budget.
pub trait Policy: Send + Sync {
    /// Whether the action is allowed for this run and context.
    fn authorize(
        &self,
        run_id: &RunId,
        action: &Action,
        ctx: &PolicyCtx,
    ) -> Result<(), KernelError>;

    /// Whether to retry after an error (and optionally after a delay).
    fn retry_strategy(
        &self,
        err: &dyn std::fmt::Display,
        _action: &Action,
    ) -> RetryDecision {
        let _ = err;
        RetryDecision::Fail
    }

    /// Retry strategy with attempt count (0-based). Default delegates to retry_strategy.
    /// Applies only to executor `Err` (infrastructure failure). `ActionResult::Failure` is not retried.
    fn retry_strategy_attempt(
        &self,
        err: &dyn std::fmt::Display,
        action: &Action,
        attempt: u32,
    ) -> RetryDecision {
        let _ = attempt;
        self.retry_strategy(err, action)
    }

    /// Optional budget; default is no limits.
    fn budget(&self) -> BudgetRules {
        BudgetRules::default()
    }
}

/// Policy that allows only actions whose tool/provider is in the given sets.
/// **Empty set = no tools or providers allowed** for that category. Sleep and WaitSignal are
/// always allowed. To allow all tools/providers use `AllowAllPolicy`, or populate the sets explicitly.
pub struct AllowListPolicy {
    pub allowed_tools: HashSet<String>,
    pub allowed_providers: HashSet<String>,
}

impl AllowListPolicy {
    pub fn new(allowed_tools: HashSet<String>, allowed_providers: HashSet<String>) -> Self {
        Self {
            allowed_tools,
            allowed_providers,
        }
    }

    pub fn tools_only(tools: impl IntoIterator<Item = String>) -> Self {
        Self {
            allowed_tools: tools.into_iter().collect(),
            allowed_providers: HashSet::new(),
        }
    }

    pub fn providers_only(providers: impl IntoIterator<Item = String>) -> Self {
        Self {
            allowed_tools: HashSet::new(),
            allowed_providers: providers.into_iter().collect(),
        }
    }
}

impl Policy for AllowListPolicy {
    fn authorize(
        &self,
        _run_id: &RunId,
        action: &Action,
        _ctx: &PolicyCtx,
    ) -> Result<(), KernelError> {
        match action {
            Action::CallTool { tool, .. } => {
                if self.allowed_tools.contains(tool) {
                    Ok(())
                } else {
                    Err(KernelError::Policy(format!("tool not allowed: {}", tool)))
                }
            }
            Action::CallLLM { provider, .. } => {
                if self.allowed_providers.contains(provider) {
                    Ok(())
                } else {
                    Err(KernelError::Policy(format!("provider not allowed: {}", provider)))
                }
            }
            Action::Sleep { .. } | Action::WaitSignal { .. } => Ok(()),
        }
    }
}

/// Policy that returns RetryAfterMs(backoff_ms) for the first max_retries attempts, then Fail.
pub struct RetryWithBackoffPolicy<P> {
    pub inner: P,
    pub max_retries: u32,
    pub backoff_ms: u64,
}

impl<P: Policy> RetryWithBackoffPolicy<P> {
    pub fn new(inner: P, max_retries: u32, backoff_ms: u64) -> Self {
        Self {
            inner,
            max_retries,
            backoff_ms,
        }
    }
}

impl<P: Policy> Policy for RetryWithBackoffPolicy<P> {
    fn authorize(&self, run_id: &RunId, action: &Action, ctx: &PolicyCtx) -> Result<(), KernelError> {
        self.inner.authorize(run_id, action, ctx)
    }

    fn retry_strategy_attempt(
        &self,
        err: &dyn std::fmt::Display,
        action: &Action,
        attempt: u32,
    ) -> RetryDecision {
        if attempt < self.max_retries {
            RetryDecision::RetryAfterMs(self.backoff_ms)
        } else {
            let _ = (err, action);
            RetryDecision::Fail
        }
    }

    fn budget(&self) -> BudgetRules {
        self.inner.budget()
    }
}
