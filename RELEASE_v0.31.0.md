# v0.31.0 - Replay-Assisted Supervised Execution Loop

`oris-runtime` now exposes a replay-aware supervised execution contract that unifies execution decision, replay outcome, fallback reason, validation status, evidence summary, and fail-closed recovery hints in one machine-readable result.

## What's in this release

- Extended `SupervisedDevloopOutcome` with unified replay-assisted execution fields, including `execution_decision`, `replay_outcome`, `fallback_reason`, `validation_outcome`, `evidence_summary`, `reason_code`, and `recovery_hint`.
- Connected `EvoKernel::run_supervised_devloop(...)` to the replay path so approved proposals can reuse existing capsules, fall back to bounded execution when replay misses safely, and fail closed when replay validation or patch reuse is unsafe.
- Added evokernel regression coverage for replay-hit reuse and replay-validation fail-closed behavior, and updated runtime wiring coverage for the new contract surface.

## Validation

- cargo fmt --all -- --check
- cargo test -p oris-evokernel --test evolution_lifecycle_regression replay_supervised_execution_ -- --nocapture
- cargo test -p oris-runtime --test evolution_feature_wiring --features full-evolution-experimental -- --nocapture
- cargo test -p oris-runtime --test agent_self_evolution_travel_network --features full-evolution-experimental -- --nocapture
- cargo test --workspace
- cargo build --verbose --all --release --all-features
- cargo test --release --all-features
- cargo publish -p oris-runtime --all-features --dry-run --registry crates-io --allow-dirty
- cargo publish -p oris-runtime --all-features --registry crates-io --allow-dirty

## Links

- Crate: https://crates.io/crates/oris-runtime
- Docs: https://docs.rs/oris-runtime
- Repo: https://github.com/Colin4k1024/Oris