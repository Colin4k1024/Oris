//! Policy: governance layer (authorize, retry, budget).
//!
//! Must exist even as a minimal implementation so Oris is not a "run any tool" demo.
//!
//! **Retry loop:** The driver calls `retry_strategy_attempt` on executor `Err` and only stops when
//! the policy returns `Fail`. Implementations must eventually return `Fail` or the loop would not
//! terminate; `RetryWithBackoffPolicy` does so after `max_retries` attempts.

use std::collections::HashSet;

use crate::kernel::action::{Action, ActionError, ActionErrorKind};
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
    fn retry_strategy(&self, err: &dyn std::fmt::Display, _action: &Action) -> RetryDecision {
        let _ = err;
        RetryDecision::Fail
    }

    /// Retry strategy with attempt count and structured error. Default uses kind: Permanent => Fail,
    /// others may be retried by implementations. Applies only to executor `Err`; `ActionResult::Failure` is not retried.
    ///
    /// `attempt` is the 0-based count of failures so far. Return `Fail` when no more retries are desired.
    fn retry_strategy_attempt(
        &self,
        err: &ActionError,
        action: &Action,
        attempt: u32,
    ) -> RetryDecision {
        let _ = (action, attempt);
        match &err.kind {
            ActionErrorKind::Permanent => RetryDecision::Fail,
            ActionErrorKind::Transient | ActionErrorKind::RateLimited => {
                // Default: no retry unless overridden
                if let ActionErrorKind::RateLimited = &err.kind {
                    if let Some(ms) = err.retry_after_ms {
                        return RetryDecision::RetryAfterMs(ms);
                    }
                }
                RetryDecision::Fail
            }
        }
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
                    Err(KernelError::Policy(format!(
                        "provider not allowed: {}",
                        provider
                    )))
                }
            }
            Action::Sleep { .. } | Action::WaitSignal { .. } => Ok(()),
        }
    }
}

/// Policy that returns RetryAfterMs with exponential backoff (and optional jitter) for the first
/// max_retries attempts, then Fail. For RateLimited errors with retry_after_ms, uses that value when set.
pub struct RetryWithBackoffPolicy<P> {
    pub inner: P,
    pub max_retries: u32,
    /// Base delay in ms; actual delay = min(cap, backoff_base_ms * 2^attempt) + jitter.
    pub backoff_base_ms: u64,
    /// Max delay cap in ms (optional).
    pub backoff_cap_ms: Option<u64>,
    /// Jitter ratio in [0.0, 1.0]; added randomness to avoid thundering herd.
    pub jitter_ratio: f64,
}

impl<P: Policy> RetryWithBackoffPolicy<P> {
    /// New with fixed backoff (no exponent, no jitter). Preserves legacy behavior when backoff_base_ms is the only delay.
    pub fn new(inner: P, max_retries: u32, backoff_ms: u64) -> Self {
        Self {
            inner,
            max_retries,
            backoff_base_ms: backoff_ms,
            backoff_cap_ms: None,
            jitter_ratio: 0.0,
        }
    }

    /// Exponential backoff: base * 2^attempt, capped, plus jitter.
    pub fn with_exponential_backoff(
        inner: P,
        max_retries: u32,
        backoff_base_ms: u64,
        backoff_cap_ms: Option<u64>,
        jitter_ratio: f64,
    ) -> Self {
        Self {
            inner,
            max_retries,
            backoff_base_ms,
            backoff_cap_ms,
            jitter_ratio,
        }
    }

    fn delay_ms(&self, err: &ActionError, attempt: u32) -> u64 {
        if matches!(err.kind, ActionErrorKind::RateLimited) && err.retry_after_ms.is_some() {
            return err.retry_after_ms.unwrap();
        }
        let exp = self.backoff_base_ms.saturating_mul(2_u64.saturating_pow(attempt));
        let capped = match self.backoff_cap_ms {
            Some(cap) => std::cmp::min(exp, cap),
            None => exp,
        };
        if self.jitter_ratio <= 0.0 {
            return capped;
        }
        // Deterministic jitter from attempt to avoid thundering herd without adding rand dep.
        let jitter_factor = ((attempt.wrapping_mul(31)) % 100) as f64 / 100.0;
        let jitter = capped as f64 * self.jitter_ratio * jitter_factor;
        (capped as f64 + jitter) as u64
    }
}

impl<P: Policy> Policy for RetryWithBackoffPolicy<P> {
    fn authorize(
        &self,
        run_id: &RunId,
        action: &Action,
        ctx: &PolicyCtx,
    ) -> Result<(), KernelError> {
        self.inner.authorize(run_id, action, ctx)
    }

    fn retry_strategy_attempt(
        &self,
        err: &ActionError,
        action: &Action,
        attempt: u32,
    ) -> RetryDecision {
        if matches!(err.kind, ActionErrorKind::Permanent) {
            return RetryDecision::Fail;
        }
        if attempt < self.max_retries {
            RetryDecision::RetryAfterMs(self.delay_ms(err, attempt))
        } else {
            let _ = action;
            RetryDecision::Fail
        }
    }

    fn budget(&self) -> BudgetRules {
        self.inner.budget()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kernel::action::ActionError;

    #[test]
    fn permanent_error_returns_fail() {
        let err = ActionError::permanent("bad request");
        let policy = AllowListPolicy::tools_only(std::iter::once("t1".to_string()));
        let action = Action::CallTool {
            tool: "t1".into(),
            input: serde_json::json!(null),
        };
        let d = policy.retry_strategy_attempt(&err, &action, 0);
        assert!(matches!(d, RetryDecision::Fail));
    }

    #[test]
    fn retry_with_backoff_transient_retries_then_fails() {
        let inner = AllowListPolicy::tools_only(std::iter::once("t1".to_string()));
        let policy = RetryWithBackoffPolicy::new(inner, 2, 10);
        let err = ActionError::transient("timeout");
        let action = Action::CallTool {
            tool: "t1".into(),
            input: serde_json::json!(null),
        };
        assert!(matches!(
            policy.retry_strategy_attempt(&err, &action, 0),
            RetryDecision::RetryAfterMs(10)
        ));
        assert!(matches!(
            policy.retry_strategy_attempt(&err, &action, 1),
            RetryDecision::RetryAfterMs(_)
        ));
        assert!(matches!(
            policy.retry_strategy_attempt(&err, &action, 2),
            RetryDecision::Fail
        ));
    }

    #[test]
    fn retry_with_backoff_rate_limited_uses_retry_after_ms() {
        let inner = AllowListPolicy::tools_only(std::iter::empty());
        let policy = RetryWithBackoffPolicy::new(inner, 3, 100);
        let err = ActionError::rate_limited("429", 2500);
        let action = Action::CallLLM {
            provider: "p1".into(),
            input: serde_json::json!(null),
        };
        let d = policy.retry_strategy_attempt(&err, &action, 0);
        assert!(matches!(d, RetryDecision::RetryAfterMs(2500)));
    }

    #[test]
    fn retry_with_backoff_exponential_increases() {
        let inner = AllowListPolicy::tools_only(std::iter::once("t1".to_string()));
        let policy = RetryWithBackoffPolicy::with_exponential_backoff(
            inner, 5, 50, Some(500), 0.0,
        );
        let err = ActionError::transient("timeout");
        let action = Action::CallTool {
            tool: "t1".into(),
            input: serde_json::json!(null),
        };
        let d0 = policy.retry_strategy_attempt(&err, &action, 0);
        let d1 = policy.retry_strategy_attempt(&err, &action, 1);
        let d2 = policy.retry_strategy_attempt(&err, &action, 2);
        let ms0 = match &d0 {
            RetryDecision::RetryAfterMs(m) => *m,
            _ => 0,
        };
        let ms1 = match &d1 {
            RetryDecision::RetryAfterMs(m) => *m,
            _ => 0,
        };
        let ms2 = match &d2 {
            RetryDecision::RetryAfterMs(m) => *m,
            _ => 0,
        };
        assert!(ms0 == 50);
        assert!(ms1 == 100);
        assert!(ms2 == 200);
    }
}
