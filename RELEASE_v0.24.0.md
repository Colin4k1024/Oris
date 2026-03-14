# v0.24.0 - Continuous Confidence Control

Harden continuous confidence control so stale or regressing self-evolution assets emit deterministic reason codes, carry auditable evidence summaries, and stay aligned through the runtime evolution facade.

## What's in this release

- Unified confidence transition evidence generation for replay-failure revocation and governor-driven confidence regression demotion, including decayed confidence, decay ratio, and phase-tagged summaries.
- Added regression assertions for stale confidence revalidation and local governor revocation so downgrade paths prove the emitted evidence contract instead of only checking terminal state.
- Exposed `TransitionEvidence` and `TransitionReasonCode` through the runtime evolution facade and locked that surface with feature wiring coverage.

## Validation

- cargo fmt --all -- --check
- cargo test -p oris-evokernel --test evolution_lifecycle_regression local_capture_uses_existing_confidence_context_for_governor -- --nocapture
- cargo test -p oris-evokernel --test evolution_lifecycle_regression stale_confidence_forces_revalidation_before_replay -- --nocapture
- cargo test -p oris-evokernel --lib
- cargo test -p oris-runtime --test evolution_feature_wiring --features full-evolution-experimental -- --nocapture
- cargo test --workspace
- cargo build --verbose --all --release --all-features
- cargo test --release --all-features

## Links

- Crate: https://crates.io/crates/oris-runtime
- Docs: https://docs.rs/oris-runtime
- Repo: https://github.com/Colin4k1024/Oris
