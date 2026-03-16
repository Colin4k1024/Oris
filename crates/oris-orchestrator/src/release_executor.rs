//! Stream F — Autonomous Release Executor with Kill-Switch enforcement.
//!
//! `ReleaseExecutor` translates an `AutonomousReleaseGateDecision` into a real
//! `cargo publish` call — or a `git revert` / `cargo yank` rollback — while
//! enforcing the kill-switch as a hard gate.
//!
//! # Pipeline position
//!
//! ```text
//! GitHubPrDeliveryAdapter  ──→  CreatedPullRequest
//!       ↓
//!  ReleaseExecutor::execute(AutonomousReleaseGateDecision)
//!       ├─ KillSwitchState::Active  → Err(KillSwitchActive)
//!       ├─ approved == false        → Err(NotApproved)
//!       ├─ publish subprocess       → Ok(ReleaseOutcome::Published)
//!       └─ rollback path            → Ok(ReleaseOutcome::RolledBack)
//! ```
//!
//! All subprocess I/O is behind `SubprocessPort` so that unit tests can
//! inject stubs without launching real child processes.
//!
//! Feature-gated behind `release-automation-experimental`.

use std::sync::Mutex;

use serde::{Deserialize, Serialize};

// ── KillSwitchState ────────────────────────────────────────────────────────

/// Hard-stop state for autonomous release operations.
///
/// When `Active`, **no** subprocess is launched and `execute()` returns
/// `Err(ReleaseExecutorError::KillSwitchActive)` immediately.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum KillSwitchState {
    /// Release operations are allowed to proceed.
    Inactive,
    /// Release operations are unconditionally blocked.
    Active,
}

// ── RollbackAction ─────────────────────────────────────────────────────────

/// A single rollback step.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum RollbackAction {
    /// Revert a git commit without auto-committing (`git revert --no-commit`).
    GitRevert {
        /// The commit SHA or ref to revert.
        commit: String,
    },
    /// Yank a published crate version from the registry.
    CargoYank {
        /// Crate name.
        package: String,
        /// Version string (e.g. `"0.5.0"`).
        version: String,
    },
}

// ── RollbackPlan ───────────────────────────────────────────────────────────

/// A sequence of rollback actions to apply when a release fails.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct RollbackPlan {
    /// When `true`, the rollback actions must be executed after a failed
    /// publish or an explicit rollback trigger.
    pub actionable: bool,
    /// Ordered list of actions to execute.
    pub actions: Vec<RollbackAction>,
    /// Human-readable reason for the rollback plan.
    pub reason: String,
}

// ── AutonomousReleaseGateDecision ──────────────────────────────────────────

/// Gate decision produced by the release gate before `ReleaseExecutor` runs.
///
/// All three fields — `kill_switch_state == Inactive`, `approved == true`, and
/// `crate_name` non-empty — must hold for a real publish to proceed.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AutonomousReleaseGateDecision {
    /// Whether the release is approved by all pre-release gates.
    pub approved: bool,
    /// Current kill-switch position.  `Active` always blocks.
    pub kill_switch_state: KillSwitchState,
    /// Crate to publish (passed to `-p` flag of `cargo publish`).
    pub crate_name: String,
    /// Version to release (for logging and yank operations).
    pub version: String,
    /// When `true`, the publish is run with `--dry-run`.
    pub dry_run: bool,
    /// Rollback plan to execute if the publish fails.
    pub rollback_plan: Option<RollbackPlan>,
}

// ── ReleaseExecutorError ───────────────────────────────────────────────────

/// Errors returned by `ReleaseExecutor::execute()`.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum ReleaseExecutorError {
    /// The kill-switch is active; no subprocess was launched.
    KillSwitchActive,
    /// The gate decision was not approved.
    NotApproved { reason: String },
    /// The crate name is missing or empty.
    MissingCrateName,
    /// `cargo publish` subprocess failed.
    PublishFailed { stderr: String },
    /// One or more rollback actions failed.
    RollbackFailed { stderr: String },
}

impl std::fmt::Display for ReleaseExecutorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::KillSwitchActive => write!(f, "KillSwitchActive: release blocked"),
            Self::NotApproved { reason } => write!(f, "NotApproved: {reason}"),
            Self::MissingCrateName => write!(f, "MissingCrateName: crate_name is empty"),
            Self::PublishFailed { stderr } => write!(f, "PublishFailed: {stderr}"),
            Self::RollbackFailed { stderr } => write!(f, "RollbackFailed: {stderr}"),
        }
    }
}

// ── ReleaseOutcome ─────────────────────────────────────────────────────────

/// Successful outcome from `ReleaseExecutor::execute()`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReleaseOutcome {
    /// Crate was published (or dry-run succeeded).
    Published {
        /// `true` when this was a dry-run only.
        dry_run: bool,
    },
    /// Rollback was triggered and completed.
    RolledBack,
}

// ── SubprocessPort ─────────────────────────────────────────────────────────

/// Port for launching child processes.
///
/// Production code uses `std::process::Command`.  Tests inject a stub.
pub trait SubprocessPort: Send + Sync {
    /// Run the command described by `args` and return stdout on success or
    /// stderr on failure.
    fn run(&self, args: &[&str]) -> Result<String, String>;
}

/// Production subprocess implementation using `std::process::Command`.
pub struct OsSubprocess;

impl SubprocessPort for OsSubprocess {
    fn run(&self, args: &[&str]) -> Result<String, String> {
        if args.is_empty() {
            return Err("empty command".to_string());
        }
        let output = std::process::Command::new(args[0])
            .args(&args[1..])
            .output()
            .map_err(|e| format!("failed to spawn {}: {e}", args[0]))?;

        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).into_owned())
        } else {
            Err(String::from_utf8_lossy(&output.stderr).into_owned())
        }
    }
}

// ── ReleaseExecutorConfig ──────────────────────────────────────────────────

/// Configuration for `ReleaseExecutor`.
#[derive(Clone, Debug)]
pub struct ReleaseExecutorConfig {
    /// Cargo registry argument (e.g. `"crates-io"`).
    pub registry: String,
    /// Extra flags forwarded to `cargo publish` (e.g. `["--allow-dirty"]`).
    pub extra_flags: Vec<String>,
}

impl Default for ReleaseExecutorConfig {
    fn default() -> Self {
        Self {
            registry: "crates-io".to_string(),
            extra_flags: Vec::new(),
        }
    }
}

// ── ReleaseExecutor ────────────────────────────────────────────────────────

/// Autonomous release executor.
///
/// Enforces the gate decision and delegates all subprocess I/O to
/// `SubprocessPort`.
pub struct ReleaseExecutor {
    subprocess: Box<dyn SubprocessPort>,
    config: ReleaseExecutorConfig,
}

impl ReleaseExecutor {
    /// Construct with an explicit subprocess port and config.
    pub fn new(subprocess: Box<dyn SubprocessPort>, config: ReleaseExecutorConfig) -> Self {
        Self { subprocess, config }
    }

    /// Convenience constructor using the real OS subprocess and default config.
    pub fn production() -> Self {
        Self::new(Box::new(OsSubprocess), ReleaseExecutorConfig::default())
    }

    /// Execute a release based on `decision`.
    ///
    /// Returns:
    /// - `Err(KillSwitchActive)` if `decision.kill_switch_state == Active`.
    /// - `Err(NotApproved)` if `decision.approved == false`.
    /// - `Err(MissingCrateName)` if `decision.crate_name` is empty.
    /// - `Ok(Published { dry_run })` on successful publish.
    /// - `Ok(RolledBack)` after a successful rollback.
    /// - `Err(PublishFailed | RollbackFailed)` on subprocess errors.
    pub fn execute(
        &self,
        decision: &AutonomousReleaseGateDecision,
    ) -> Result<ReleaseOutcome, ReleaseExecutorError> {
        // 1. Kill-switch — hard block before any subprocess.
        if decision.kill_switch_state == KillSwitchState::Active {
            return Err(ReleaseExecutorError::KillSwitchActive);
        }

        // 2. Approval gate.
        if !decision.approved {
            return Err(ReleaseExecutorError::NotApproved {
                reason: "gate decision is not approved".to_string(),
            });
        }

        // 3. Crate name guard.
        if decision.crate_name.trim().is_empty() {
            return Err(ReleaseExecutorError::MissingCrateName);
        }

        // 4. Build publish command.
        let dry_run = decision.dry_run;
        let publish_result = self.run_publish(decision, dry_run);

        match publish_result {
            Ok(_) => Ok(ReleaseOutcome::Published { dry_run }),
            Err(stderr) => {
                // 5. Rollback path.
                if let Some(ref plan) = decision.rollback_plan {
                    if plan.actionable {
                        return self.run_rollback(plan).map(|_| ReleaseOutcome::RolledBack);
                    }
                }
                Err(ReleaseExecutorError::PublishFailed { stderr })
            }
        }
    }

    fn run_publish(
        &self,
        decision: &AutonomousReleaseGateDecision,
        dry_run: bool,
    ) -> Result<String, String> {
        let mut args = vec![
            "cargo".to_string(),
            "publish".to_string(),
            "-p".to_string(),
            decision.crate_name.clone(),
            "--all-features".to_string(),
            "--registry".to_string(),
            self.config.registry.clone(),
        ];

        if dry_run {
            args.push("--dry-run".to_string());
        }

        for flag in &self.config.extra_flags {
            args.push(flag.clone());
        }

        let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
        self.subprocess.run(&arg_refs)
    }

    fn run_rollback(&self, plan: &RollbackPlan) -> Result<(), ReleaseExecutorError> {
        for action in &plan.actions {
            let result = match action {
                RollbackAction::GitRevert { commit } => {
                    self.subprocess
                        .run(&["git", "revert", "--no-commit", commit])
                }
                RollbackAction::CargoYank { package, version } => self
                    .subprocess
                    .run(&["cargo", "yank", "--vers", version, package]),
            };
            if let Err(stderr) = result {
                return Err(ReleaseExecutorError::RollbackFailed { stderr });
            }
        }
        Ok(())
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Stub subprocess ──────────────────────────────────────────────────

    struct StubSubprocess {
        /// If `Some(err)` the next call returns that error; otherwise returns
        /// `Ok("ok")`.  `None` means always succeed.
        error: Option<String>,
        calls: Mutex<Vec<Vec<String>>>,
    }

    impl StubSubprocess {
        fn ok() -> Self {
            Self {
                error: None,
                calls: Mutex::new(vec![]),
            }
        }
        fn err(reason: &str) -> Self {
            Self {
                error: Some(reason.to_string()),
                calls: Mutex::new(vec![]),
            }
        }
        fn call_count(&self) -> usize {
            self.calls.lock().unwrap_or_else(|p| p.into_inner()).len()
        }
        fn last_args(&self) -> Vec<String> {
            self.calls
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .last()
                .cloned()
                .unwrap_or_default()
        }
    }

    impl SubprocessPort for StubSubprocess {
        fn run(&self, args: &[&str]) -> Result<String, String> {
            self.calls
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .push(args.iter().map(|s| s.to_string()).collect());
            match &self.error {
                Some(e) => Err(e.clone()),
                None => Ok("ok".to_string()),
            }
        }
    }

    // ── Helpers ──────────────────────────────────────────────────────────

    fn approved_decision() -> AutonomousReleaseGateDecision {
        AutonomousReleaseGateDecision {
            approved: true,
            kill_switch_state: KillSwitchState::Inactive,
            crate_name: "oris-orchestrator".to_string(),
            version: "0.5.0".to_string(),
            dry_run: false,
            rollback_plan: None,
        }
    }

    fn executor(subprocess: StubSubprocess) -> ReleaseExecutor {
        ReleaseExecutor::new(Box::new(subprocess), ReleaseExecutorConfig::default())
    }

    // ── release_automation_kill_switch_active_blocks_publish ─────────────

    #[test]
    fn release_automation_kill_switch_active_blocks_publish() {
        let stub = StubSubprocess::ok();
        let ex = executor(stub);

        let decision = AutonomousReleaseGateDecision {
            kill_switch_state: KillSwitchState::Active,
            ..approved_decision()
        };

        let result = ex.execute(&decision);
        assert_eq!(result, Err(ReleaseExecutorError::KillSwitchActive));
    }

    // ── release_automation_kill_switch_active_no_subprocess_launched ─────

    #[test]
    fn release_automation_kill_switch_active_no_subprocess_launched() {
        let stub = StubSubprocess::ok();
        // Capture call_count before moving stub into executor.
        let calls = std::sync::Arc::new(Mutex::new(0usize));
        let calls_clone = std::sync::Arc::clone(&calls);

        struct CountingSubprocess(std::sync::Arc<Mutex<usize>>);
        impl SubprocessPort for CountingSubprocess {
            fn run(&self, _args: &[&str]) -> Result<String, String> {
                *self.0.lock().unwrap() += 1;
                Ok("ok".to_string())
            }
        }
        let _ = stub; // drop unused
        let ex = ReleaseExecutor::new(
            Box::new(CountingSubprocess(calls_clone)),
            ReleaseExecutorConfig::default(),
        );

        let decision = AutonomousReleaseGateDecision {
            kill_switch_state: KillSwitchState::Active,
            ..approved_decision()
        };
        let _ = ex.execute(&decision);
        assert_eq!(*calls.lock().unwrap(), 0, "no subprocess must be launched");
    }

    // ── release_automation_approved_executes_cargo_publish ───────────────

    #[test]
    fn release_automation_approved_executes_cargo_publish() {
        let stub = StubSubprocess::ok();
        let ex = executor(stub);

        let result = ex.execute(&approved_decision());
        assert_eq!(result, Ok(ReleaseOutcome::Published { dry_run: false }));
    }

    // ── release_automation_approved_publish_args_contain_crate_name ──────

    #[test]
    fn release_automation_approved_publish_args_contain_crate_name() {
        let stub = StubSubprocess::ok();
        // We need to read calls after execute, so use Arc<Mutex> pattern.
        let calls: std::sync::Arc<Mutex<Vec<Vec<String>>>> =
            std::sync::Arc::new(Mutex::new(vec![]));
        let calls_clone = std::sync::Arc::clone(&calls);

        struct RecordingSubprocess(std::sync::Arc<Mutex<Vec<Vec<String>>>>);
        impl SubprocessPort for RecordingSubprocess {
            fn run(&self, args: &[&str]) -> Result<String, String> {
                self.0
                    .lock()
                    .unwrap()
                    .push(args.iter().map(|s| s.to_string()).collect());
                Ok("ok".to_string())
            }
        }
        let _ = stub;
        let ex = ReleaseExecutor::new(
            Box::new(RecordingSubprocess(calls_clone)),
            ReleaseExecutorConfig::default(),
        );

        ex.execute(&approved_decision()).unwrap();

        let recorded = calls.lock().unwrap();
        assert!(!recorded.is_empty());
        let args = &recorded[0];
        assert!(
            args.contains(&"oris-orchestrator".to_string()),
            "crate name must appear in publish args: {args:?}"
        );
        assert!(
            args.contains(&"publish".to_string()),
            "cargo publish expected"
        );
    }

    // ── release_automation_dry_run_uses_dry_run_flag ─────────────────────

    #[test]
    fn release_automation_dry_run_uses_dry_run_flag() {
        let calls: std::sync::Arc<Mutex<Vec<Vec<String>>>> =
            std::sync::Arc::new(Mutex::new(vec![]));
        let calls_clone = std::sync::Arc::clone(&calls);

        struct RecordingSubprocess(std::sync::Arc<Mutex<Vec<Vec<String>>>>);
        impl SubprocessPort for RecordingSubprocess {
            fn run(&self, args: &[&str]) -> Result<String, String> {
                self.0
                    .lock()
                    .unwrap()
                    .push(args.iter().map(|s| s.to_string()).collect());
                Ok("ok".to_string())
            }
        }
        let ex = ReleaseExecutor::new(
            Box::new(RecordingSubprocess(calls_clone)),
            ReleaseExecutorConfig::default(),
        );

        let decision = AutonomousReleaseGateDecision {
            dry_run: true,
            ..approved_decision()
        };
        let result = ex.execute(&decision);
        assert_eq!(result, Ok(ReleaseOutcome::Published { dry_run: true }));

        let recorded = calls.lock().unwrap();
        let args = &recorded[0];
        assert!(
            args.contains(&"--dry-run".to_string()),
            "--dry-run must be included: {args:?}"
        );
    }

    // ── release_automation_rollback_executed_when_actionable ─────────────

    #[test]
    fn release_automation_rollback_executed_when_actionable() {
        // First call (publish) fails; second call (rollback) succeeds.
        let call_counter: std::sync::Arc<Mutex<usize>> = std::sync::Arc::new(Mutex::new(0usize));
        let counter_clone = std::sync::Arc::clone(&call_counter);

        struct FailFirstSubprocess(std::sync::Arc<Mutex<usize>>);
        impl SubprocessPort for FailFirstSubprocess {
            fn run(&self, _args: &[&str]) -> Result<String, String> {
                let mut count = self.0.lock().unwrap();
                *count += 1;
                if *count == 1 {
                    Err("error: publish forbidden".to_string())
                } else {
                    Ok("ok".to_string())
                }
            }
        }

        let ex = ReleaseExecutor::new(
            Box::new(FailFirstSubprocess(counter_clone)),
            ReleaseExecutorConfig::default(),
        );

        let decision = AutonomousReleaseGateDecision {
            rollback_plan: Some(RollbackPlan {
                actionable: true,
                actions: vec![RollbackAction::GitRevert {
                    commit: "abc123".to_string(),
                }],
                reason: "publish failed, reverting".to_string(),
            }),
            ..approved_decision()
        };

        let result = ex.execute(&decision);
        assert_eq!(result, Ok(ReleaseOutcome::RolledBack));
        assert_eq!(
            *call_counter.lock().unwrap(),
            2,
            "two subprocess calls expected"
        );
    }

    // ── release_automation_not_approved_blocks_publish ───────────────────

    #[test]
    fn release_automation_not_approved_blocks_publish() {
        let ex = executor(StubSubprocess::ok());

        let decision = AutonomousReleaseGateDecision {
            approved: false,
            ..approved_decision()
        };
        let result = ex.execute(&decision);
        assert!(
            matches!(result, Err(ReleaseExecutorError::NotApproved { .. })),
            "not-approved decision must return NotApproved error"
        );
    }

    // ── release_automation_missing_crate_name_blocks_publish ─────────────

    #[test]
    fn release_automation_missing_crate_name_blocks_publish() {
        let ex = executor(StubSubprocess::ok());

        let decision = AutonomousReleaseGateDecision {
            crate_name: "   ".to_string(),
            ..approved_decision()
        };
        let result = ex.execute(&decision);
        assert_eq!(result, Err(ReleaseExecutorError::MissingCrateName));
    }

    // ── release_automation_publish_error_without_rollback_plan ───────────

    #[test]
    fn release_automation_publish_error_without_rollback_plan() {
        let ex = executor(StubSubprocess::err("cargo: crate not found"));

        let result = ex.execute(&approved_decision());
        assert!(
            matches!(result, Err(ReleaseExecutorError::PublishFailed { .. })),
            "publish error must surface as PublishFailed"
        );
    }

    // ── release_automation_cargo_yank_rollback_action ────────────────────

    #[test]
    fn release_automation_cargo_yank_rollback_action() {
        let call_args: std::sync::Arc<Mutex<Vec<Vec<String>>>> =
            std::sync::Arc::new(Mutex::new(vec![]));
        let args_clone = std::sync::Arc::clone(&call_args);

        struct FailFirstRecord(std::sync::Arc<Mutex<Vec<Vec<String>>>>);
        impl SubprocessPort for FailFirstRecord {
            fn run(&self, args: &[&str]) -> Result<String, String> {
                let mut all = self.0.lock().unwrap();
                let call_num = all.len();
                all.push(args.iter().map(|s| s.to_string()).collect());
                if call_num == 0 {
                    Err("publish failed".to_string())
                } else {
                    Ok("ok".to_string())
                }
            }
        }

        let ex = ReleaseExecutor::new(
            Box::new(FailFirstRecord(args_clone.clone())),
            ReleaseExecutorConfig::default(),
        );

        let decision = AutonomousReleaseGateDecision {
            rollback_plan: Some(RollbackPlan {
                actionable: true,
                actions: vec![RollbackAction::CargoYank {
                    package: "oris-orchestrator".to_string(),
                    version: "0.5.0".to_string(),
                }],
                reason: "yank on failure".to_string(),
            }),
            ..approved_decision()
        };

        let result = ex.execute(&decision);
        assert_eq!(result, Ok(ReleaseOutcome::RolledBack));

        let recorded = args_clone.lock().unwrap();
        let yank_args = &recorded[1];
        assert!(
            yank_args.contains(&"yank".to_string()),
            "yank action must call cargo yank: {yank_args:?}"
        );
        assert!(yank_args.contains(&"0.5.0".to_string()));
    }
}
