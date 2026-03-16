# Release: oris-runtime v0.37.0

## Summary

Implements **AUTO-05 — Continuous Confidence Revalidation** for the autonomous
evolution pipeline.

Assets (genes and capsules) can now be continuously re-evaluated for replay
eligibility as runtime confidence signals accumulate. Failed revalidation
suspends replay automatically; assets with too many failures are demoted or
quarantined.

## New Types (`oris-agent-contract v0.5.3`)

| Type | Kind | Description |
|------|------|-------------|
| `ConfidenceState` | `enum` | Asset lifecycle states: `Active`, `Decaying`, `Revalidating`, `Demoted`, `Quarantined` |
| `RevalidationOutcome` | `enum` | Round outcome: `Passed`, `Failed`, `Pending`, `ErrorFailClosed` |
| `ConfidenceDemotionReasonCode` | `enum` | Reason codes for demotion: `ConfidenceDecayThreshold`, `RepeatedReplayFailure`, `MaxFailureCountExceeded`, `ExplicitRevocation`, `UnknownFailClosed` |
| `ReplayEligibility` | `enum` | `Eligible` / `Ineligible` after evaluation |
| `ConfidenceRevalidationResult` | `struct` | Full result of one revalidation run |
| `DemotionDecision` | `struct` | Asset demotion / quarantine transition record |

## New Constructors (`oris-agent-contract v0.5.3`)

| Constructor | Description |
|-------------|-------------|
| `pass_confidence_revalidation(…)` | Creates a passing result, restores `Active` state |
| `fail_confidence_revalidation(…)` | Creates a failing result, marks asset `Ineligible`, `fail_closed=true` |
| `demote_asset(…)` | Creates demotion decision; escalates to `Quarantined` when appropriate |

## New Kernel Methods (`oris-evokernel v0.12.4`)

| Method | Description |
|--------|-------------|
| `EvoKernel::evaluate_confidence_revalidation(asset_id, state, failures)` | Run a revalidation cycle; fail-closed at 3+ failures |
| `EvoKernel::evaluate_asset_demotion(asset_id, prior_state, failures, reason)` | Produce demotion decision; quarantine at 5+ failures |

## Policy

- **Pass threshold**: `failure_count < 3` → `Passed`, `Eligible`, `fail_closed=false`
- **Fail threshold**: `failure_count >= 3` → `Failed`, `Ineligible`, `fail_closed=true`
- **Demote vs Quarantine**: `failure_count < 5` → `Demoted`; `failure_count >= 5` → `Quarantined`

## Validation

- `cargo fmt --all -- --check` ✓
- 5 regression tests (`confidence_revalidation_*`) — all pass
- 1 wiring gate test (`confidence_revalidation_decision_types_resolve`) — pass
- `cargo build --all --release --all-features` ✓
- `cargo test --release --all-features` ✓
- `cargo publish -p oris-runtime --all-features --dry-run` ✓

## Changed Crates

| Crate | Old | New |
|-------|-----|-----|
| `oris-agent-contract` | 0.5.2 | 0.5.3 |
| `oris-evokernel` | 0.12.3 | 0.12.4 |
| `oris-runtime` | 0.36.0 | 0.37.0 |

## Linked Issue

Closes #269
