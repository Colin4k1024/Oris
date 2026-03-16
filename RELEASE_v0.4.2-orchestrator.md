# Release: oris-orchestrator v0.4.2

**Issue**: #284 — EVO26-AUTO Stream E: PR Automation GitHub API Integration

## Summary

Adds `GitHubPrDeliveryAdapter` in `crates/oris-orchestrator/src/github_delivery.rs`
— a real `PrDeliveryPort` implementation that gates PR delivery on credentials,
polls CI check-runs, and applies a configurable merge allow-list before squash-merging.

## Changes

### New: `crates/oris-orchestrator/src/github_delivery.rs`

- **`AutonomousPrLaneStatus`** — `PrReady | PrBlocked { reason } | PrPending`.
- **`AutonomousPrLaneDecision`** — upstream gate decision carrying `pr_ready: bool`,
  `lane_status`, `branch_name`, and `pr_payload`.
- **`CiCheckStatus`** — `Passed | Pending | Failed | TimedOut`.
- **`PrCreationPort`** — trait: `create(payload) -> (pr_number, sha)`.
- **`CiCheckPort`** — trait: `check(owner, repo, sha) -> CiCheckStatus`.
- **`MergePort`** — trait: `squash_merge(owner, repo, pr_number) -> Result`.
- **`GitHubDeliveryConfig`** — configures token env var, `token` override,
  `ci_poll_interval` (default 30s), `ci_timeout` (default 10 min),
  `merge_allow_list`, and `auto_merge_all`.
- **`GitHubPrDeliveryAdapter`** — implements `PrDeliveryPort`:
  1. Credential gate: `ORIS_GITHUB_TOKEN` must be set; fails with
     `MissingCredentials` otherwise.
  2. PR creation via `PrCreationPort`.
  3. Blocking CI poll loop with configurable timeout.
  4. Squash-merge when CI passes **and** `issue_id` matches `merge_allow_list`.

### Modified: `crates/oris-orchestrator/src/lib.rs`

- Added `pub mod github_delivery;`.

## Version Bump

`oris-orchestrator`: `0.4.1` → `0.4.2` (patch — new module, no breaking changes).

## Tests Added (9 tests in `github_delivery::tests`)

- `pr_automation_missing_token_returns_missing_credentials`
- `pr_automation_ci_pass_triggers_merge`
- `pr_automation_ci_fail_no_merge`
- `pr_automation_ci_timeout_no_merge`
- `pr_automation_disallowed_class_no_merge`
- `pr_automation_pr_creation_error_propagates`
- `pr_automation_allowed_class_triggers_merge`
- `pr_automation_lane_decision_pr_ready_true`
- `pr_automation_lane_decision_pr_blocked`
