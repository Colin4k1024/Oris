# v0.30.0 - Structured Mutation Proposal Contracts

`oris-runtime` now exposes structured self-evolution mutation proposal contracts that declare bounded scope, validation budget, approval requirements, expected evidence, and fail-closed rejection semantics before execution begins.

## What's in this release

- Added machine-readable self-evolution mutation proposal contracts to the experimental agent contract surface, including `proposal_scope`, `validation_budget`, `approval_required`, `expected_evidence`, `reason_code`, and `fail_closed`.
- Added `EvoKernel::prepare_self_evolution_mutation_proposal(...)` and pre-execution proposal validation so malformed or out-of-bounds supervised mutations are rejected before execution starts.
- Extended evokernel regression coverage and runtime feature wiring coverage for accepted proposal generation, fail-closed scope rejection, and missing target-file rejection.

## Validation

- cargo fmt --all -- --check
- cargo test -p oris-evokernel --test evolution_lifecycle_regression mutation_proposal_ -- --nocapture
- cargo test -p oris-runtime --test evolution_feature_wiring --features full-evolution-experimental -- --nocapture
- cargo test --workspace
- cargo build --verbose --all --release --all-features
- cargo test --release --all-features
- cargo publish -p oris-runtime --all-features --dry-run --registry crates-io --allow-dirty
- cargo publish -p oris-runtime --all-features --registry crates-io --allow-dirty

## Links

- Crate: https://crates.io/crates/oris-runtime
- Docs: https://docs.rs/oris-runtime
- Repo: https://github.com/Colin4k1024/Oris
