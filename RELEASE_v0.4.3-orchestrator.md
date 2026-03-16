# Release: oris-orchestrator v0.4.3

**Crate:** `oris-orchestrator`
**Version:** `0.4.3`
**Issue:** [#285 — EVO26-AUTO Stream F: Release Automation](https://github.com/orisai/orisai/issues/285)

## Summary

Adds `release_executor` module (behind `release-automation-experimental` feature flag) that implements
the Stream F gate for autonomous release: kill-switch pre-check, approved-decision gate, `cargo publish`
subprocess dispatch, and actionable rollback execution when publish fails.

## New Items

### `release_executor` module (feature-gated)

- **`KillSwitchState`** — `Active | Inactive` enum controlling whether any publish may proceed.
- **`RollbackAction`** — `GitRevert { commit }` | `CargoYank { package, version }` rollback primitives.
- **`RollbackPlan`** — `actionable: bool`, `actions: Vec<RollbackAction>`, `reason: String`.
- **`AutonomousReleaseGateDecision`** — Decision struct carrying `approved`, `kill_switch_state`,
  `crate_name`, `version`, `dry_run`, and an optional `rollback_plan`.
- **`ReleaseExecutorError`** — `KillSwitchActive | NotApproved | MissingCrateName | PublishFailed { stderr } | RollbackFailed { stderr }`.
- **`ReleaseOutcome`** — `Published { dry_run } | RolledBack`.
- **`SubprocessPort`** — Injectable trait for subprocess execution (testable without real `cargo`).
- **`OsSubprocess`** — Production implementation using `std::process::Command`.
- **`ReleaseExecutorConfig`** — `registry: String`, `extra_flags: Vec<String>`.
- **`ReleaseExecutor::execute()`** — Enforces kill-switch → approved → non-empty crate name → publish → rollback pipeline.

### Tests (10 new, all pass)

All prefixed `release_automation_*`:
- `kill_switch_active_blocks_publish`
- `kill_switch_active_no_subprocess_launched`
- `approved_executes_cargo_publish`
- `approved_publish_args_contain_crate_name`
- `dry_run_uses_dry_run_flag`
- `rollback_executed_when_actionable`
- `not_approved_blocks_publish`
- `missing_crate_name_blocks_publish`
- `publish_error_without_rollback_plan`
- `cargo_yank_rollback_action`

## Validation

```
cargo fmt --all -- --check          ✅
cargo build --all --release --all-features   ✅
cargo test --release --all-features ✅
cargo publish -p oris-orchestrator --all-features --dry-run  ✅
```
