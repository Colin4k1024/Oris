# Evo A2A Semi-Autonomous Release Readiness

Date: 2026-03-05
Milestone: `Evo A2A Semi-Autonomous Release`
Scope: issues `#96` through `#104`

## Passed Commands

- `bash scripts/run_orchestrator_checks.sh`
- `cargo test -p oris-orchestrator`
- `cargo test -p oris-runtime --test evolution_feature_wiring --features full-evolution-experimental`

## Release-Gate Behavior Proof

- State machine gate is enforced:
  - `release_requires_explicit_approval_path` verifies `Merged -> ReleasePendingApproval` only through `request_release`.
- Explicit human approval gate is enforced:
  - `publish_requires_explicit_approval` verifies publish is allowed only for `ReleaseDecision::Approved`.
- Coordinator stops before publish:
  - `flow_reaches_release_pending_approval_before_publish` verifies the coordinator returns `ReleasePendingApproval`.
- Validation evidence gate requires full green:
  - `pr_ready_requires_full_green_validation` verifies PR readiness requires `build_ok && tests_ok && policy_ok`.

## Known Limitations

- The orchestrator modules in this milestone are contract-first skeletons; they are not yet wired to live GitHub or runtime A2A services.
- `Coordinator::run_single_issue` currently returns a deterministic placeholder state for testability and does not execute remote orchestration.
- Validation and release gates operate on in-memory model values only; no persisted approval ledger is included in this milestone.
