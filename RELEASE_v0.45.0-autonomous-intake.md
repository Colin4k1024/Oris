# Release Notes — oris-runtime v0.45.0 (Autonomous Candidate Intake)

**Crate**: `oris-runtime`
**Version**: 0.44.0 → **0.45.0** (minor — new public API surface)
**Issue**: [#321 EVO26-AUTO-01][P1] Autonomous Candidate Intake From CI and Runtime Signals

## Summary

Implements the first layer of the autonomous self-evolution pipeline (`EVO26-AUTO-01`). The runtime can now discover and classify work candidates directly from CI diagnostic signals without any caller-supplied issue metadata.

## Changes

### `oris-runtime` 0.44.0 → 0.45.0

#### New API: `EvoKernel::discover_autonomous_candidates`

```rust
pub fn discover_autonomous_candidates(
    &self,
    input: &AutonomousIntakeInput,
) -> AutonomousIntakeOutput
```

- Classifies raw diagnostic signals from `AutonomousCandidateSource` variants (`CiFailure`, `TestRegression`, `CompileRegression`, `LintRegression`, `RuntimeIncident`) without requiring a caller-supplied issue number.
- **Deduplication**: Normalised signals are hashed into a stable `dedupe_key`. Candidates whose key already appears in the evolution store window are denied with `reason_code: duplicate_candidate`.
- **Fail-closed on unsupported/ambiguous signals**: Returns an explicit `reason_code` (`ambiguous_signal` or `unsupported_signal_class`) — never silently dropped.
- **Empty batch guard**: Returns a single denied candidate with `reason_code: unknown_fail_closed` for an empty `raw_signals` slice.

#### Contract Types (via `oris-agent-contract`, re-exported through `oris-runtime`)

| Type | Description |
|------|-------------|
| `AutonomousIntakeInput` | Raw signal batch and source classification |
| `AutonomousIntakeOutput` | Accepted + denied candidate list with aggregate counts |
| `DiscoveredCandidate` | Stable `dedupe_key`, `candidate_source`, `candidate_class`, `signals`, `accepted`, `reason_code` |
| `AutonomousCandidateSource` | `CiFailure \| TestRegression \| CompileRegression \| LintRegression \| RuntimeIncident` |
| `AutonomousIntakeReasonCode` | `Accepted \| UnsupportedSignalClass \| AmbiguousSignal \| DuplicateCandidate \| UnknownFailClosed` |

#### Regression Tests (`evolution_lifecycle_regression`)

| Test | Coverage |
|------|----------|
| `autonomous_intake_accepts_compile_regression_signal` | Compile signal → accepted, `CompileRegression` source |
| `autonomous_intake_accepts_test_failure_signal` | Test failure → accepted, `TestRegression` source |
| `autonomous_intake_deduplicates_equivalent_signals` | Known `dedupe_key` → denied, `DuplicateCandidate` |
| `autonomous_intake_denies_empty_signals_fail_closed` | Empty batch → denied, `UnknownFailClosed` |
| `autonomous_intake_denies_ambiguous_signals_fail_closed` | Unclassifiable signals → denied, `AmbiguousSignal` |

## Validation

- `cargo fmt --all -- --check` ✅
- `cargo test -p oris-evokernel --test evolution_lifecycle_regression autonomous_intake_` → **5 passed** ✅
- `cargo test -p oris-runtime --test evolution_feature_wiring --features full-evolution-experimental` → **10 passed** ✅
- `cargo build --all --release --all-features` ✅
- `cargo test --release --all-features` ✅
- `cargo publish -p oris-runtime --all-features --dry-run` ✅

## Closes

- #321 [EVO26-AUTO-01][P1] Autonomous Candidate Intake From CI and Runtime Signals
