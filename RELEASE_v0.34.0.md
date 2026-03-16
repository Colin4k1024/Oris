# Release Notes — oris-runtime v0.34.0

## Summary

Implements **EVO26-AUTO-02: Bounded Task Planning and Risk Scoring For Autonomous Intake** (Issue #265).

Adds a machine-readable planning contract that scores every `DiscoveredCandidate` produced by
AUTO-01 intake before any mutation is proposed. The planner assigns a risk tier, feasibility
score, and expected validation budget, and rejects high-risk or low-confidence work fail-closed.

## Changes

### `oris-agent-contract` (0.5.0 — unchanged)

New public types in the autonomous-intake planning surface:

| Symbol | Kind | Description |
|--------|------|-------------|
| `AutonomousRiskTier` | `enum` | `Low / Medium / High` — orderable risk classification |
| `AutonomousPlanReasonCode` | `enum` | Stable discriminants: `Approved`, `DeniedHighRisk`, `DeniedLowFeasibility`, `DeniedUnsupportedClass`, `DeniedNoEvidence`, `UnknownFailClosed` |
| `AutonomousDenialCondition` | `struct` | Structured denial: `reason_code`, `description`, `recovery_hint` |
| `AutonomousTaskPlan` | `struct` | Full planning output: plan_id, dedupe_key, task_class, risk_tier, feasibility_score, validation_budget, expected_evidence, approved, reason_code, summary, denial_condition, fail_closed |
| `approve_autonomous_task_plan(…)` | `fn` | Constructor for approved plans |
| `deny_autonomous_task_plan(…)` | `fn` | Constructor for denied fail-closed plans |

### `oris-evokernel` (0.12.1 — unchanged)

- `EvoKernel::plan_autonomous_candidate(&DiscoveredCandidate) -> AutonomousTaskPlan` — new public method
- Internal: `autonomous_plan_for_candidate` + `autonomous_planning_params_for_class` private helpers
- Risk policy: `High` risk tier → denied; feasibility < 40 → denied; all denials are fail-closed

### Class–Risk mapping

| `BoundedTaskClass` | `AutonomousRiskTier` | Feasibility | Budget |
|--------------------|---------------------|-------------|--------|
| `LintFix` | Low | 85 | 2 |
| `DocsSingleFile` | Low | 90 | 1 |
| `DocsMultiFile` | Medium | 75 | 2 |
| `CargoDepUpgrade` | Medium | 70 | 3 |

## Tests

- 5 new regression tests in `oris-evokernel/tests/evolution_lifecycle_regression.rs` (prefix `autonomous_planning_`)
- 1 new wiring gate test `autonomous_task_planning_types_resolve` in `oris-runtime/tests/evolution_feature_wiring.rs`

## Validation

```
cargo fmt --all -- --check               ✓
cargo build --all --release --all-features  ✓
cargo test --release --all-features      ✓  (0 failures)
cargo publish -p oris-runtime --all-features --dry-run  ✓
```

## Closes

- Issue #265: `[EVO26-AUTO-02][P1] Bounded Task Planning and Risk Scoring For Autonomous Intake`
