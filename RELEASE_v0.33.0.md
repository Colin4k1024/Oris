# Release Notes — oris-runtime v0.33.0

## Summary

Implements **EVO26-AUTO-01**: Autonomous Candidate Intake From CI and Runtime Signals.

This release adds the first machine-readable contract for discovering evolution
candidates autonomously from CI and runtime diagnostic signals, without requiring
a caller-supplied GitHub issue number.

## Changes

### New: Autonomous Intake Contract (`oris-agent-contract`)

Five new public types:

| Type | Purpose |
|------|---------|
| `AutonomousCandidateSource` | Signal source classification (`CiFailure`, `TestRegression`, `CompileRegression`, `LintRegression`, `RuntimeIncident`) |
| `AutonomousIntakeReasonCode` | Outcome reason code (`Accepted`, `UnsupportedSignalClass`, `AmbiguousSignal`, `DuplicateCandidate`, `UnknownFailClosed`) |
| `DiscoveredCandidate` | A single discovered candidate with `dedupe_key`, `candidate_source`, `candidate_class`, `signals`, `reason_code`, `fail_closed` |
| `AutonomousIntakeInput` | Input to the intake method: `source_id`, `candidate_source`, `raw_signals` |
| `AutonomousIntakeOutput` | Batch output: `candidates`, `accepted_count`, `denied_count` |

Two new helper constructors:

- `accept_discovered_candidate(dedupe_key, source, class, signals, summary)` → `DiscoveredCandidate`
- `deny_discovered_candidate(dedupe_key, source, signals, reason_code)` → `DiscoveredCandidate`

### New: `EvoKernel::discover_autonomous_candidates` (`oris-evokernel`)

New method on `EvoKernel<S>`:

```rust
pub fn discover_autonomous_candidates(
    &self,
    input: &AutonomousIntakeInput,
) -> AutonomousIntakeOutput
```

Behavior:
- Normalises raw signals (trim, lowercase, sort, dedup)
- Computes a stable `dedupe_key` via `stable_hash_json`
- Checks for duplicate candidates in the evolution store (via `SignalsExtracted` events)
- Maps signal source to `BoundedTaskClass` (`LintFix` for compile/test/lint/CI; `None` for `RuntimeIncident`)
- Fail-closes on empty signals, ambiguous sources, or unknown outcomes
- Does **not** generate mutation proposals, trigger task planning, or run executors

### Validation

- `cargo test -p oris-evokernel --test evolution_lifecycle_regression autonomous_intake_` — 5 new tests, all passing
- `cargo test -p oris-runtime --test evolution_feature_wiring --features full-evolution-experimental` — 4 tests, all passing
- `cargo build --all --release --all-features` — clean build
- `cargo test --release --all-features` — no failures

## Non-Goals (not included in this release)

- Mutation proposal generation from discovered candidates
- Autonomous execution or task planning
- PR/release automation triggered by intake
- Recovery or incident escalation beyond fail-closed signaling

## Upgrade Notes

No breaking changes. New types are additive. Existing `select_self_evolution_candidate` and `prepare_self_evolution_mutation_proposal` are unchanged.
