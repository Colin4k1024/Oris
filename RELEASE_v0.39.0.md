# Release v0.39.0

## Summary

EVO26-AUTO-07: Fail-Closed Autonomous Merge and Release Gate For Narrow Safe Lanes.

## Changes

### `oris-agent-contract` v0.5.5

- Added `AutonomousMergeGateStatus` enum: `MergeApproved`, `MergeBlocked`
- Added `AutonomousReleaseGateStatus` enum: `ReleaseApproved`, `ReleaseBlocked`
- Added `AutonomousPublishGateStatus` enum: `PublishApproved`, `PublishBlocked`
- Added `KillSwitchState` enum: `Inactive`, `Active`
- Added `AutonomousReleaseReasonCode` enum with 7 reason codes
- Added `RollbackPlan` struct
- Added `AutonomousReleaseGateDecision` struct (machine-readable gate decision record)
- Added `approve_autonomous_release_gate()` constructor
- Added `deny_autonomous_release_gate()` constructor

### `oris-evokernel` v0.12.6

- Added `EvoKernel::evaluate_autonomous_release_gate()` public method
- Added `autonomous_release_gate_decision()` private helper (fail-closed policy: only `DocsSingleFile` and `LintFix` at `Low` risk tier with inactive kill switch and complete evidence are approved)

### `oris-orchestrator` v0.3.2

- Updated `oris-agent-contract` dependency to v0.5.5
- Added `tests/autonomous_release_gate.rs` with 5 regression tests

### `oris-runtime` v0.39.0

- Updated `oris-evokernel` dependency to v0.12.6
- Added `autonomous_release_gate_decision_types_resolve` wiring gate test

## Validation

- `cargo fmt --all -- --check`
- `cargo test -p oris-orchestrator autonomous_release_ -- --nocapture` — 5 tests passing
- `cargo test -p oris-runtime --test evolution_feature_wiring --features full-evolution-experimental autonomous_release_gate_decision_types_resolve` — passing
- `cargo test --release --all-features` — 0 failures
- `cargo publish -p oris-runtime --all-features --dry-run` — passed
