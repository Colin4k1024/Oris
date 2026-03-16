# Release: oris-runtime v0.38.0

## Summary

Implements **AUTO-06 — Bounded Autonomous PR Lane For Low-Risk Task Classes**.

A narrowly scoped autonomous PR lane is now available for explicitly approved low-risk task classes (`DocsSingleFile`, `LintFix`). The lane gates on validated evidence; all other classes and missing evidence fail closed before any PR payload is assembled.

## New Types (`oris-agent-contract v0.5.4`)

| Type | Kind | Description |
|------|------|-------------|
| `AutonomousPrLaneStatus` | `enum` | `PrReady` / `Denied` |
| `PrLaneApprovalState` | `enum` | `ClassApproved` / `ClassNotApproved` |
| `AutonomousPrLaneReasonCode` | `enum` | `ApprovedForAutonomousPr`, `TaskClassNotApproved`, `PatchEvidenceMissing`, `ValidationEvidenceMissing`, `RiskTierTooHigh`, `UnknownFailClosed` |
| `PrEvidenceBundle` | `struct` | Evidence gate: patch summary, validation status, audit trail |
| `AutonomousPrLaneDecision` | `struct` | Full PR lane decision record with `branch_name`, `pr_payload`, `evidence_bundle` |

## New Constructors (`oris-agent-contract v0.5.4`)

| Constructor | Description |
|-------------|-------------|
| `approve_autonomous_pr_lane(…)` | Creates an approved decision with branch and PR payload |
| `deny_autonomous_pr_lane(…)` | Creates a denied decision, always `fail_closed=true` |

## New Kernel Method (`oris-evokernel v0.12.5`)

| Method | Description |
|--------|-------------|
| `EvoKernel::evaluate_autonomous_pr_lane(task_id, task_class, risk_tier, evidence_bundle)` | Gate for the bounded autonomous PR lane |

## Policy

- **Approved classes**: `DocsSingleFile`, `LintFix` at `AutonomousRiskTier::Low` with `validation_passed=true`
- **All other configurations**: `Denied`, `fail_closed=true`

## Validation

- `cargo fmt --all -- --check` ✓
- 5 regression tests (`autonomous_pr_lane_*`) — all pass
- 1 wiring gate test (`autonomous_pr_lane_decision_types_resolve`) — pass
- `cargo build --all --release --all-features` ✓
- `cargo test --release --all-features` ✓
- `cargo publish -p oris-runtime --all-features --dry-run` ✓

## Changed Crates

| Crate | Old | New |
|-------|-----|-----|
| `oris-agent-contract` | 0.5.3 | 0.5.4 |
| `oris-evokernel` | 0.12.4 | 0.12.5 |
| `oris-runtime` | 0.37.0 | 0.38.0 |

## Linked Issue

Closes #270
