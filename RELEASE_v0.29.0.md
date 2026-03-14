# v0.29.0 - Self-Evolution Candidate Intake Contracts

`oris-runtime` now exposes a bounded GitHub issue-style self-evolution candidate intake path with machine-readable accept/reject decisions and fail-closed reason codes.

Also shipped:

- `oris-agent-contract v0.4.0`
- `oris-evokernel v0.11.0`

## What's in this release

- Added `SelfEvolutionCandidateIntakeRequest`, `SelfEvolutionSelectionReasonCode`, and `SelfEvolutionSelectionDecision` to the public agent contract surface.
- Added `EvoKernel::select_self_evolution_candidate(...)` so bounded GitHub issue-shaped candidates can be accepted or rejected before proposal generation.
- Locked accept, reject, and fail-closed selection behavior with evokernel regressions and runtime facade wiring coverage.

## Validation

- cargo fmt --all -- --check
- cargo test -p oris-evokernel --test evolution_lifecycle_regression candidate_intake_ -- --nocapture
- cargo test -p oris-runtime --test evolution_feature_wiring --features full-evolution-experimental -- --nocapture
- cargo test --workspace
- cargo build --verbose --all --release --all-features
- cargo test --release --all-features

## Links

- Crate: https://crates.io/crates/oris-runtime
- Docs: https://docs.rs/oris-runtime
- Repo: https://github.com/Colin4k1024/Oris
