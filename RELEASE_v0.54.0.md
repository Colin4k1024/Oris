# Release v0.54.0

## Summary

Adds the fail-closed Autonomous Merge and Release Gate for narrow approved task
classes (issue #327, Stream G EVO26-AUTO-07).

The new `oris_orchestrator::autonomous_release` module defines three sequential
gate contracts that must all pass before any autonomous merge or publish action
is taken:

- **`MergeGate`** — enforces kill switch, class eligibility (narrow approved
  set only), risk-tier ceiling, and complete evidence across all pipeline
  stages (intake → planning → proposal → execution → confidence → PR).
- **`ExtendedReleaseGate`** — re-checks kill switch, verifies the merge gate
  result, and rejects any post-gate state drift.
- **`GatedPublishGate`** — re-checks kill switch, requires release gate
  approval, and mandates a validated `RollbackPlan` before allowing any
  publish.

All three gates are fail-closed: `fail_closed = true` is unconditionally set
in every result type.  No gate ever silently passes.

### Machine-readable outputs

Every result struct carries the required machine-readable fields:
`merge_gate_result`, `release_gate_result`, `publish_gate_result`,
`kill_switch_state`, `rollback_plan`, `reason_code`, `fail_closed`.

### Approved task classes for autonomous merge

Only the three narrowest, lowest-risk task classes are eligible:
`missing-import`, `type-mismatch`, `test-failure`.

High-risk or unrecognised task classes are denied with
`ReleaseReasonCode::IneligibleClass`.

### Kill switch

`KillSwitchState` has three variants:
- `Inactive` — automation may proceed.
- `Active` — manually halted; all gates deny immediately.
- `IncidentTripped { incident_id }` — automatically tripped by an incident
  stop condition; all gates deny immediately.

### Rollback hooks

`RollbackPlan` captures a `restore_ref` (git tag or SHA), an ordered list of
rollback steps, and a `validated` flag.  The publish gate rejects any plan
that is absent, has no steps, or has not been validated.

## Changed crates

| Crate | Old | New | Bump reason |
|---|---|---|---|
| `oris-runtime` | 0.53.0 | 0.54.0 | minor — new public module `autonomous_release` in the orchestration boundary |
| `oris-orchestrator` | 0.4.3 | 0.5.0 | minor — new public module with new gate contracts |

## Validation

- `cargo fmt --all -- --check` ✓
- `cargo test -p oris-orchestrator autonomous_release_` — 44 new tests, all pass ✓
- `cargo test -p oris-orchestrator` — 91 lib + integration tests pass ✓
- `cargo build --all --release --all-features` ✓
- `cargo test --release --all-features` — 0 failures ✓
- `cargo publish -p oris-runtime --all-features --dry-run` ✓

## Non-goals (unchanged)

- No unconstrained autonomous release.
- No high-risk class auto-merge.
- No weakening of existing fail-closed `AcceptanceGate` semantics.
- No hidden publish path outside the explicit release gate.
