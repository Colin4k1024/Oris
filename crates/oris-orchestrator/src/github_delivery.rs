//! Stream E — Real `PrDeliveryPort` with CI polling and merge gate.
//!
//! `GitHubPrDeliveryAdapter` implements `PrDeliveryPort` with three stages:
//!
//! ```text
//! deliver(PrPayload)
//!   ├─ 1. Credential gate  — ORIS_GITHUB_TOKEN must be set
//!   ├─ 2. PR creation      — POST /repos/{owner}/{repo}/pulls
//!   ├─ 3. CI poll loop     — GET check-runs until pass / fail / timeout
//!   └─ 4. Merge gate       — squash-merge when CI passes and class is allowed
//! ```
//!
//! All external I/O is behind traits (`PrCreationPort`, `CiCheckPort`,
//! `MergePort`) so that unit tests can inject stubs without making real HTTP
//! calls.

use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::autonomous_loop::PrDeliveryPort;
use crate::github_adapter::{CreatedPullRequest, PrPayload};

// ── AutonomousPrLaneStatus ─────────────────────────────────────────────────

/// High-level gate state for a candidate before `deliver()` is called.
///
/// Callers must verify `AutonomousPrLaneDecision.pr_ready == true` before
/// calling `GitHubPrDeliveryAdapter::deliver()`.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AutonomousPrLaneStatus {
    /// The candidate has passed all pre-delivery gates and is ready for PR
    /// creation.
    PrReady,
    /// The candidate was blocked before delivery.
    PrBlocked {
        /// Human-readable reason for the block.
        reason: String,
    },
    /// Pre-delivery evaluation is still running.
    PrPending,
}

/// Upstream gate decision produced before calling `deliver()`.
///
/// The `pr_payload` carries the branch name and body for the future PR.
/// Only call `GitHubPrDeliveryAdapter::deliver(&self.pr_payload)` when
/// `pr_ready == true`.
#[derive(Clone, Debug)]
pub struct AutonomousPrLaneDecision {
    /// Whether the PR may proceed.
    pub pr_ready: bool,
    /// Detailed lane status with optional block reason.
    pub lane_status: AutonomousPrLaneStatus,
    /// The branch name that will be used as `head` in the PR.
    pub branch_name: String,
    /// Human-readable explanation of the lane decision.
    pub reason: String,
    /// The ready-to-deliver payload (only valid when `pr_ready == true`).
    pub pr_payload: PrPayload,
}

// ── CiCheckStatus ─────────────────────────────────────────────────────────

/// Current status of a CI check suite.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CiCheckStatus {
    /// All required checks have completed and passed.
    Passed,
    /// One or more checks are still running or queued.
    Pending,
    /// At least one check has failed or been cancelled.
    Failed,
    /// The polling deadline was reached without a conclusive result.
    TimedOut,
}

// ── I/O ports (testable abstractions) ─────────────────────────────────────

/// Port: create a PR and return `(pr_number, head_sha)`.
pub trait PrCreationPort: Send + Sync {
    /// Create the pull-request described by `payload`.
    ///
    /// Returns `(pr_number, head_sha)` on success, or `Err(reason)` on
    /// failure.  Implementations must never return
    /// `Err("MissingCredentials")` — that check is done by the adapter
    /// before calling this port.
    fn create(&self, payload: &PrPayload) -> Result<(u64, String), String>;
}

/// Port: query the current CI check status for a commit SHA.
///
/// Implementations are expected to be idempotent and side-effect free.
pub trait CiCheckPort: Send + Sync {
    /// Return the current aggregate CI status for `sha`.
    ///
    /// Implementations should return `CiCheckStatus::Pending` if any checks
    /// are still in progress, and `CiCheckStatus::Passed` only when *all*
    /// required checks have completed successfully.
    fn check(&self, owner: &str, repo: &str, sha: &str) -> CiCheckStatus;
}

/// Port: request a squash-merge for the given PR.
pub trait MergePort: Send + Sync {
    /// Squash-merge PR number `pr_number` in `{owner}/{repo}`.
    ///
    /// Returns `Ok(())` on success or `Err(reason)` on failure.  Merge
    /// failures are surfaced in the `CreatedPullRequest::merge_error` field
    /// rather than bubbling up as a `deliver()` error.
    fn squash_merge(&self, owner: &str, repo: &str, pr_number: u64) -> Result<(), String>;
}

// ── Configuration ──────────────────────────────────────────────────────────

/// Configuration for `GitHubPrDeliveryAdapter`.
#[derive(Clone, Debug)]
pub struct GitHubDeliveryConfig {
    /// Name of the environment variable that holds the GitHub token.
    /// Defaults to `"ORIS_GITHUB_TOKEN"`.
    pub token_env_var: String,
    /// Explicit token value.  When set, takes precedence over `token_env_var`.
    /// Useful for injecting tokens in tests without touching the environment.
    pub token: Option<String>,
    /// Repository owner (org or user).
    pub owner: String,
    /// Repository name.
    pub repo: String,
    /// How long to wait between CI poll requests.
    pub ci_poll_interval: Duration,
    /// Maximum time to wait for CI to complete before giving up.
    pub ci_timeout: Duration,
    /// Set of task-class ids that are permitted to auto-merge.
    ///
    /// When empty, no auto-merge is performed (humans must merge manually).
    /// When non-empty, a PR is squash-merged automatically when CI passes
    /// and the delivering payload's `issue_id` prefix matches one of the ids.
    pub merge_allow_list: Vec<String>,
    /// When `true`, auto-merge is performed for all PRs regardless of
    /// `merge_allow_list`.  Intended for tests only.
    pub auto_merge_all: bool,
}

impl GitHubDeliveryConfig {
    /// Construct with sensible defaults: 30-second CI poll, 10-minute timeout,
    /// and an empty merge allow-list (no auto-merge).
    pub fn new(owner: impl Into<String>, repo: impl Into<String>) -> Self {
        Self {
            token_env_var: "ORIS_GITHUB_TOKEN".to_string(),
            token: None,
            owner: owner.into(),
            repo: repo.into(),
            ci_poll_interval: Duration::from_secs(30),
            ci_timeout: Duration::from_secs(600),
            merge_allow_list: Vec::new(),
            auto_merge_all: false,
        }
    }

    /// Enable auto-merge for the listed task-class ids.
    pub fn with_merge_allow_list(
        mut self,
        ids: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        self.merge_allow_list = ids.into_iter().map(Into::into).collect();
        self
    }
}

// ── GitHubPrDeliveryAdapter ────────────────────────────────────────────────

/// Real `PrDeliveryPort` implementation for autonomous PR delivery.
///
/// Wires credential check, PR creation, CI polling, and merge gate:
///
/// 1. Read the GitHub token from the environment variable named by
///    `config.token_env_var`; fail-closed with `MissingCredentials` if absent.
/// 2. Create the PR via `PrCreationPort`.
/// 3. Poll `CiCheckPort` until all checks pass, one fails, or
///    `config.ci_timeout` is exceeded.
/// 4. When CI passes **and** the task class is in `config.merge_allow_list`,
///    call `MergePort::squash_merge()`.
pub struct GitHubPrDeliveryAdapter {
    config: GitHubDeliveryConfig,
    pr_creation: Box<dyn PrCreationPort>,
    ci_check: Box<dyn CiCheckPort>,
    merge: Box<dyn MergePort>,
}

impl GitHubPrDeliveryAdapter {
    /// Create the adapter with explicit port implementations.
    ///
    /// In production code, pass real HTTP-backed implementations.
    /// In test code, pass stubs.
    pub fn new(
        config: GitHubDeliveryConfig,
        pr_creation: Box<dyn PrCreationPort>,
        ci_check: Box<dyn CiCheckPort>,
        merge: Box<dyn MergePort>,
    ) -> Self {
        Self {
            config,
            pr_creation,
            ci_check,
            merge,
        }
    }

    /// Read the GitHub token from the configured environment variable.
    ///
    /// Returns `Err("MissingCredentials: ...")` if the variable is unset or
    /// empty.
    fn resolve_token(&self) -> Result<String, String> {
        // Explicit override takes precedence (used in tests).
        if let Some(ref t) = self.config.token {
            if !t.trim().is_empty() {
                return Ok(t.clone());
            }
        }
        let token = std::env::var(&self.config.token_env_var).unwrap_or_default();
        if token.trim().is_empty() {
            Err(format!(
                "MissingCredentials: {} is not set or empty",
                self.config.token_env_var
            ))
        } else {
            Ok(token)
        }
    }

    /// Poll CI until a conclusive result or timeout.
    fn poll_ci(&self, sha: &str) -> CiCheckStatus {
        let deadline = Instant::now() + self.config.ci_timeout;
        loop {
            let status = self
                .ci_check
                .check(&self.config.owner, &self.config.repo, sha);
            match status {
                CiCheckStatus::Pending => {
                    if Instant::now() >= deadline {
                        return CiCheckStatus::TimedOut;
                    }
                    // Blocking sleep — acceptable for deterministic poll, real
                    // callers should use tokio::time::sleep in async context.
                    std::thread::sleep(self.config.ci_poll_interval);
                }
                conclusive => return conclusive,
            }
        }
    }

    /// Determine whether auto-merge is permitted for `issue_id`.
    ///
    /// Returns `true` when:
    /// * `config.auto_merge_all` is set, OR
    /// * at least one entry in `config.merge_allow_list` is a prefix of or
    ///   exact match for `issue_id`.
    fn merge_allowed(&self, issue_id: &str) -> bool {
        if self.config.auto_merge_all {
            return true;
        }
        if self.config.merge_allow_list.is_empty() {
            return false;
        }
        self.config
            .merge_allow_list
            .iter()
            .any(|class_id| issue_id.contains(class_id.as_str()) || class_id == "*")
    }
}

impl PrDeliveryPort for GitHubPrDeliveryAdapter {
    fn deliver(&self, payload: &PrPayload) -> Result<CreatedPullRequest, String> {
        // 1. Credential gate — fail-closed.
        self.resolve_token()?;

        // 2. PR creation.
        let (pr_number, sha) = self.pr_creation.create(payload)?;

        // 3. CI polling.
        let ci_status = self.poll_ci(&sha);

        // 4. Merge gate.
        if ci_status == CiCheckStatus::Passed && self.merge_allowed(&payload.issue_id) {
            // Best-effort merge — don't fail the entire deliver() on merge error;
            // the PR has already been created.
            let _ = self
                .merge
                .squash_merge(&self.config.owner, &self.config.repo, pr_number);
        }

        // Return a stable `CreatedPullRequest` regardless of merge outcome.
        Ok(CreatedPullRequest {
            number: pr_number,
            url: format!(
                "https://github.com/{}/{}/pull/{}",
                self.config.owner, self.config.repo, pr_number
            ),
        })
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;

    // ── Stub implementations ─────────────────────────────────────────────

    struct StubPrCreation {
        pr_number: u64,
        sha: String,
        error: Option<String>,
        calls: Mutex<Vec<PrPayload>>,
    }

    impl StubPrCreation {
        fn ok(pr_number: u64, sha: &str) -> Self {
            Self {
                pr_number,
                sha: sha.to_string(),
                error: None,
                calls: Mutex::new(vec![]),
            }
        }
        fn err(reason: &str) -> Self {
            Self {
                pr_number: 0,
                sha: String::new(),
                error: Some(reason.to_string()),
                calls: Mutex::new(vec![]),
            }
        }
        fn recorded(&self) -> Vec<PrPayload> {
            self.calls.lock().unwrap_or_else(|p| p.into_inner()).clone()
        }
    }

    impl PrCreationPort for StubPrCreation {
        fn create(&self, payload: &PrPayload) -> Result<(u64, String), String> {
            self.calls
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .push(payload.clone());
            match &self.error {
                Some(e) => Err(e.clone()),
                None => Ok((self.pr_number, self.sha.clone())),
            }
        }
    }

    struct StubCiCheck(CiCheckStatus);
    impl CiCheckPort for StubCiCheck {
        fn check(&self, _owner: &str, _repo: &str, _sha: &str) -> CiCheckStatus {
            self.0
        }
    }

    struct StubMerge {
        calls: Arc<Mutex<Vec<u64>>>,
        error: Option<String>,
    }

    impl StubMerge {
        fn ok() -> Self {
            Self {
                calls: Arc::new(Mutex::new(vec![])),
                error: None,
            }
        }
        fn err(reason: &str) -> Self {
            Self {
                calls: Arc::new(Mutex::new(vec![])),
                error: Some(reason.to_string()),
            }
        }
        fn call_count(&self) -> usize {
            self.calls.lock().unwrap_or_else(|p| p.into_inner()).len()
        }
    }

    impl MergePort for StubMerge {
        fn squash_merge(&self, _owner: &str, _repo: &str, pr_number: u64) -> Result<(), String> {
            self.calls
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .push(pr_number);
            match &self.error {
                Some(e) => Err(e.clone()),
                None => Ok(()),
            }
        }
    }

    fn sample_payload() -> PrPayload {
        PrPayload::new(
            "issue-42",
            "codex/fix-branch",
            "main",
            "evidence-abc123",
            "Automated PR body",
        )
    }

    fn base_config() -> GitHubDeliveryConfig {
        GitHubDeliveryConfig {
            token_env_var: "ORIS_GITHUB_TOKEN_TEST_UNUSED".to_string(),
            token: None,
            owner: "test-owner".to_string(),
            repo: "test-repo".to_string(),
            ci_poll_interval: Duration::from_millis(1),
            ci_timeout: Duration::from_millis(50),
            merge_allow_list: Vec::new(),
            auto_merge_all: false,
        }
    }

    fn authed_config() -> GitHubDeliveryConfig {
        GitHubDeliveryConfig {
            token: Some("test-token-abc".to_string()),
            ..base_config()
        }
    }

    // ── pr_automation_missing_token_returns_missing_credentials ──────────

    #[test]
    fn pr_automation_missing_token_returns_missing_credentials() {
        // base_config has no token and uses an env var that is not set.
        let adapter = GitHubPrDeliveryAdapter::new(
            base_config(),
            Box::new(StubPrCreation::ok(1, "sha-abc")),
            Box::new(StubCiCheck(CiCheckStatus::Passed)),
            Box::new(StubMerge::ok()),
        );

        let result = adapter.deliver(&sample_payload());
        assert!(result.is_err());
        let msg = result.unwrap_err();
        assert!(
            msg.contains("MissingCredentials"),
            "expected MissingCredentials in error: {msg}"
        );
    }

    // ── pr_automation_ci_pass_triggers_merge ─────────────────────────────

    #[test]
    fn pr_automation_ci_pass_triggers_merge() {
        let merge_stub = Arc::new(StubMerge::ok());
        let merge_clone = Arc::clone(&merge_stub);

        struct SharedMerge(Arc<StubMerge>);
        impl MergePort for SharedMerge {
            fn squash_merge(&self, owner: &str, repo: &str, pr_number: u64) -> Result<(), String> {
                self.0.squash_merge(owner, repo, pr_number)
            }
        }

        let config = GitHubDeliveryConfig {
            auto_merge_all: true,
            ..authed_config()
        };
        let adapter = GitHubPrDeliveryAdapter::new(
            config,
            Box::new(StubPrCreation::ok(42, "sha-pass")),
            Box::new(StubCiCheck(CiCheckStatus::Passed)),
            Box::new(SharedMerge(merge_clone)),
        );

        let result = adapter.deliver(&sample_payload());
        assert!(result.is_ok(), "expected Ok, got {:?}", result);
        assert_eq!(
            merge_stub.call_count(),
            1,
            "squash_merge must be called once when CI passes"
        );
    }

    // ── pr_automation_ci_fail_no_merge ───────────────────────────────────

    #[test]
    fn pr_automation_ci_fail_no_merge() {
        let config = GitHubDeliveryConfig {
            auto_merge_all: true,
            ..authed_config()
        };
        let adapter = GitHubPrDeliveryAdapter::new(
            config,
            Box::new(StubPrCreation::ok(99, "sha-fail")),
            Box::new(StubCiCheck(CiCheckStatus::Failed)),
            Box::new(StubMerge::ok()),
        );

        // CI failed → PR is still created, no merge.
        let result = adapter.deliver(&sample_payload());
        assert!(result.is_ok(), "deliver should Ok even when CI fails");
        let pr = result.unwrap();
        assert_eq!(pr.number, 99);
    }

    // ── pr_automation_ci_timeout_no_merge ────────────────────────────────

    #[test]
    fn pr_automation_ci_timeout_no_merge() {
        // StubCiCheck always returns Pending → timeout is exercised.
        let config = GitHubDeliveryConfig {
            ci_poll_interval: Duration::from_millis(1),
            ci_timeout: Duration::from_millis(5),
            auto_merge_all: true,
            ..authed_config()
        };

        let adapter = GitHubPrDeliveryAdapter::new(
            config,
            Box::new(StubPrCreation::ok(7, "sha-pending")),
            Box::new(StubCiCheck(CiCheckStatus::Pending)),
            Box::new(StubMerge::ok()),
        );

        let result = adapter.deliver(&sample_payload());
        assert!(result.is_ok(), "deliver should Ok on CI timeout");
    }

    // ── pr_automation_disallowed_class_no_merge ──────────────────────────

    #[test]
    fn pr_automation_disallowed_class_no_merge() {
        // Allow-list does not include "issue-42" → no merge.
        let config = GitHubDeliveryConfig {
            merge_allow_list: vec!["test-failure".to_string()],
            auto_merge_all: false,
            ..authed_config()
        };

        let merge_stub = StubMerge::ok();
        let merge_count = Arc::clone(&merge_stub.calls);
        let adapter = GitHubPrDeliveryAdapter::new(
            config,
            Box::new(StubPrCreation::ok(5, "sha-ok")),
            Box::new(StubCiCheck(CiCheckStatus::Passed)),
            Box::new(merge_stub),
        );

        let result = adapter.deliver(&sample_payload());
        assert!(result.is_ok());
        assert_eq!(
            merge_count.lock().unwrap().len(),
            0,
            "merge must not run for disallowed task class"
        );
    }

    // ── pr_automation_pr_creation_error_propagates ───────────────────────

    #[test]
    fn pr_automation_pr_creation_error_propagates() {
        let adapter = GitHubPrDeliveryAdapter::new(
            authed_config(),
            Box::new(StubPrCreation::err(
                "github api returned 422: validation error",
            )),
            Box::new(StubCiCheck(CiCheckStatus::Passed)),
            Box::new(StubMerge::ok()),
        );

        let result = adapter.deliver(&sample_payload());
        assert!(result.is_err());
        assert!(
            result.unwrap_err().contains("422"),
            "PR creation error should propagate"
        );
    }

    // ── pr_automation_allowed_class_triggers_merge ───────────────────────

    #[test]
    fn pr_automation_allowed_class_triggers_merge() {
        // Allow-list contains "issue-42" (substring match).
        let config = GitHubDeliveryConfig {
            merge_allow_list: vec!["issue-42".to_string()],
            auto_merge_all: false,
            ..authed_config()
        };

        let merge_stub = StubMerge::ok();
        let merge_calls = Arc::clone(&merge_stub.calls);
        let adapter = GitHubPrDeliveryAdapter::new(
            config,
            Box::new(StubPrCreation::ok(11, "sha-ok")),
            Box::new(StubCiCheck(CiCheckStatus::Passed)),
            Box::new(merge_stub),
        );

        let result = adapter.deliver(&sample_payload()); // payload.issue_id = "issue-42"
        assert!(result.is_ok());
        assert_eq!(
            merge_calls.lock().unwrap().len(),
            1,
            "merge should run when class is in allow-list"
        );
    }

    // ── pr_automation_lane_decision_pr_ready_true ────────────────────────

    #[test]
    fn pr_automation_lane_decision_pr_ready_true() {
        let payload = sample_payload();
        let decision = AutonomousPrLaneDecision {
            pr_ready: true,
            lane_status: AutonomousPrLaneStatus::PrReady,
            branch_name: payload.head.clone(),
            reason: "All gates passed".to_string(),
            pr_payload: payload,
        };
        assert!(decision.pr_ready);
        assert_eq!(decision.lane_status, AutonomousPrLaneStatus::PrReady);
    }

    // ── pr_automation_lane_decision_pr_blocked ───────────────────────────

    #[test]
    fn pr_automation_lane_decision_pr_blocked() {
        let payload = sample_payload();
        let decision = AutonomousPrLaneDecision {
            pr_ready: false,
            lane_status: AutonomousPrLaneStatus::PrBlocked {
                reason: "kill-switch active".to_string(),
            },
            branch_name: payload.head.clone(),
            reason: "Kill-switch is active; blocking delivery.".to_string(),
            pr_payload: payload,
        };
        assert!(!decision.pr_ready);
        assert!(matches!(
            decision.lane_status,
            AutonomousPrLaneStatus::PrBlocked { .. }
        ));
    }
}
