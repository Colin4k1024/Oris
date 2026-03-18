# v0.51.0 - Semantic Replay Task-Class Generalization

oris-runtime now exposes deterministic semantic replay decisions for bounded task families so replay can generalize beyond exact normalized signals without allowing false-positive reuse for unrelated work.

## What's in this release

- Added semantic replay decision contracts for task equivalence class, equivalence explanation, replay confidence, reason code, and fail-closed outcomes.
- Added EvoKernel semantic replay evaluation for approved low-risk task families and deterministic denial for medium-risk families that require human review.
- Kept the semantic replay surface wired through the runtime facade and covered by regression tests for auditability and stable machine-readable enums.

## Validation

- cargo test -p oris-evokernel --test evolution_lifecycle_regression semantic_replay_ -- --nocapture
- cargo test -p oris-runtime --test evolution_feature_wiring --features full-evolution-experimental -- --nocapture
- cargo test -p oris-runtime --test agent_self_evolution_travel_network --features full-evolution-experimental -- --nocapture
- cargo test --workspace
- cargo fmt --all -- --check
- cargo build --verbose --all --release --all-features
- cargo test --release --all-features
- cargo publish -p oris-runtime --all-features --dry-run
- cargo publish -p oris-runtime --all-features

## Links

- Crate: https://crates.io/crates/oris-runtime
- Docs: https://docs.rs/oris-runtime
- Repo: https://github.com/Colin4k1024/Oris