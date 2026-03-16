# Release v0.4.0 — oris-orchestrator

## Summary

Implements Stream B: Autonomous Task Planning Chain (Issue #280).

### Changes

- **New module `task_planner`**: `BoundedTaskClass`, `RiskTier`, `FeasibilityScore`, `BlastRadius`,
  `AutonomousPlanReasonCode`, `AutonomousTaskPlan`, and `plan_autonomous_candidate()`.
- **Real `ClassifierPort` implementation**: `BoundedClassifier` backed by `TaskClassMatcher` from
  `oris-evolution`, replacing the `FixedClassifier` test stub in production usage.
- **`AutonomousLoop` integration**: `process_issue()` now chains intake output to task planning
  before proposal generation. High-risk and low-feasibility candidates are denied fail-closed with
  `AutonomousPlanReasonCode::DeniedHighRisk` (or `DeniedLowFeasibility`, `DeniedBlastRadiusExceeded`,
  `DeniedUnknownClass`) and produce `IssueOutcome::PlanDenied` without external side effects.
- **New `IssueOutcome::PlanDenied`** variant carrying a structured `reason_code` string.
- **`AutonomousLoop::with_bounded_classes()`** builder method for test-overridable class registry.

### Validation

- `cargo fmt --all -- --check` ✓
- `cargo test -p oris-orchestrator task_planning_` — 8/8 passed ✓
- `cargo test --workspace` — 0 failures ✓
- `cargo publish -p oris-orchestrator --dry-run` passed ✓

## Crate

`oris-orchestrator` v0.4.0
