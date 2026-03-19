# Release Notes — oris-runtime v0.46.0 (Autonomous Task Planning)

**Crate**: `oris-runtime`
**Version**: 0.45.0 → **0.46.0** (minor — new public API surface)
**Issue**: [#322 EVO26-AUTO-02][P1] Bounded Task Planning and Risk Scoring For Autonomous Intake

## Summary

Implements the second layer of the autonomous self-evolution pipeline (`EVO26-AUTO-02`). The runtime can now convert a `DiscoveredCandidate` into an auditable, machine-readable `AutonomousTaskPlan` with risk tier, feasibility score, validation budget, and expected evidence. High-risk or unsupported candidates are denied fail-closed before any proposal generation begins.

## Changes

### `oris-runtime` 0.45.0 → 0.46.0

#### New API: `EvoKernel::plan_autonomous_candidate`

```rust
pub fn plan_autonomous_candidate(
    &self,
    candidate: &DiscoveredCandidate,
) -> AutonomousTaskPlan
```

- Classifies a `DiscoveredCandidate` into a `task_class` (reuses `BoundedTaskClass`).
- Assigns `risk_tier` and `feasibility_score` based on candidate class and signal count.
- Attaches `validation_budget` and required `expected_evidence` list.
- Denied candidates (not accepted) produce a fail-closed plan with `reason_code: DeniedCandidate`.
- Missing `candidate_class` produces a fail-closed plan with `reason_code: UnsupportedTaskClass`.

#### Contract Types (via `oris-agent-contract`, re-exported through `oris-runtime`)

| Type | Description |
|------|-------------|
| `AutonomousTaskPlan` | Full planning record: `plan_id`, `dedupe_key`, `task_class`, `risk_tier`, `feasibility_score`, `validation_budget`, `expected_evidence`, `approved`, `reason_code` |
| `AutonomousPlanReasonCode` | `Approved \| DeniedCandidate \| UnsupportedTaskClass \| UnknownFailClosed` |
| `AutonomousRiskTier` | `Low \| Medium \| High` |

#### Regression Tests (`evolution_lifecycle_regression`)

| Test | Coverage |
|------|----------|
| `autonomous_planning_approves_lint_fix_candidate` | LintFix candidate → approved plan |
| `autonomous_planning_approves_docs_single_file_candidate` | DocsSingleFile → approved plan |
| `autonomous_planning_denies_denied_candidate_fail_closed` | Denied candidate → denied plan, `DeniedCandidate` |
| `autonomous_planning_denies_missing_class_fail_closed` | No task class → denied plan, `UnsupportedTaskClass` |
| `autonomous_planning_reason_codes_are_stable` | Reason codes stable across equivalent classes |

## Validation

- `cargo fmt --all -- --check` ✅
- `cargo test -p oris-evokernel --test evolution_lifecycle_regression autonomous_planning_` → **5 passed** ✅
- `cargo test -p oris-runtime --test evolution_feature_wiring --features full-evolution-experimental` → **10 passed** ✅

## Closes

- #322 [EVO26-AUTO-02][P1] Bounded Task Planning and Risk Scoring For Autonomous Intake
