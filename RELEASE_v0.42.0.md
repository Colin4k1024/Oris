# v0.42.0 - Intake-Driven Detect Stage Integration

oris-runtime now exposes intake-to-detect pipeline wiring so runtime diagnostics and webhook-derived failures can feed the evolution loop through the standard detect stage.

## What's in this release

- Added intake-driven detect-stage integration in oris-evokernel via `detect_from_intake_events` and `intake_events_to_extractor_input`.
- Added compiler-diagnostic and runtime-panic coverage for the detect-to-select path, including runtime facade resolution under `full-evolution-experimental`.

## Validation

- cargo fmt --all -- --check
- cargo test -p oris-evokernel --test evolution_feature_wiring
- cargo test -p oris-evokernel
- cargo test -p oris-runtime --test evolution_feature_wiring --features full-evolution-experimental
- cargo build --verbose --all --release --all-features
- cargo test --release --all-features

## Links

- Crate: https://crates.io/crates/oris-runtime
- Docs: https://docs.rs/oris-runtime
- Repo: https://github.com/Colin4k1024/Oris