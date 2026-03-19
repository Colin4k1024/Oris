# v0.52.0 - Continuous Confidence Revalidation and Asset Demotion

oris-runtime now exposes continuous confidence revalidation and deterministic asset demotion decisions so stale or repeatedly failing reusable assets automatically lose replay eligibility.

## What's in this release

- Added machine-readable confidence lifecycle contracts covering confidence state, revalidation result, replay eligibility, demotion decision, quarantine transition, and stable reason codes.
- Added EvoKernel entrypoints for confidence revalidation and asset demotion with deterministic escalation from demotion to quarantine based on failure count.
- Kept the confidence lifecycle surface wired through the runtime facade and covered by regression tests for auditability and replay-safety enforcement.

## Validation

- cargo test -p oris-evokernel --test evolution_lifecycle_regression confidence_revalidation_ -- --nocapture
- cargo test -p oris-runtime --test evolution_feature_wiring --features full-evolution-experimental -- --nocapture
- cargo test -p oris-runtime --test agent_self_evolution_travel_network --features full-evolution-experimental -- --nocapture
- cargo test --workspace
- cargo fmt --all -- --check
- cargo build --verbose --all --release --all-features
- cargo test --release --all-features
- cargo publish -p oris-runtime --all-features --registry crates-io --dry-run
- cargo publish -p oris-runtime --all-features --registry crates-io

## Links

- Crate: https://crates.io/crates/oris-runtime
- Docs: https://docs.rs/oris-runtime
- Repo: https://github.com/Colin4k1024/Oris