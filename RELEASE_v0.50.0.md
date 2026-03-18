# v0.50.0 - Autonomous Mutation Proposal Contracts

oris-runtime now turns approved autonomous task plans into bounded, machine-readable mutation proposals that stay compatible with the supervised execution path while failing closed on malformed or weak proposals.

## What's in this release

- Added autonomous mutation proposal generation through the EvoKernel autonomous proposal entrypoint for approved bounded plans.
- Added bounded proposal scope, expected evidence, rollback conditions, approval mode, and stable fail-closed reason codes for denied proposals.
- Kept the generated proposal shape aligned with the existing supervised execution contract surface and runtime feature wiring.

## Validation

- cargo fmt --all -- --check
- cargo test -p oris-evokernel --test evolution_lifecycle_regression autonomous_proposal_ -- --nocapture
- cargo test -p oris-runtime --test evolution_feature_wiring --features full-evolution-experimental -- --nocapture
- cargo test --workspace
- cargo build --verbose --all --release --all-features
- cargo test --release --all-features
- cargo publish -p oris-runtime --all-features --dry-run
- cargo publish -p oris-runtime --all-features

## Links

- Crate: https://crates.io/crates/oris-runtime
- Docs: https://docs.rs/oris-runtime
- Repo: https://github.com/Colin4k1024/Oris
