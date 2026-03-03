# v0.10.0 - Governor rate limits and confidence decay

`oris-runtime` now ships richer EvoKernel governor policies for mutation pacing, retry cooldowns, and confidence-based regression handling.

## What's in this release

- Added time-window mutation rate limits and retry cooldown controls to the EvoKernel governor so rapid successive mutations can be deferred instead of promoted immediately.
- Added confidence decay and confidence-history-based regression revocation, with new regression coverage in both `oris-governor` and EvoKernel black-box tests.

## Validation

- cargo fmt --all -- --check
- cargo test -p oris-governor -- --nocapture
- cargo test -p oris-evokernel --test evolution_lifecycle_regression -- --nocapture
- cargo test -p oris-runtime --test evolution_feature_wiring -- --nocapture
- cargo test --workspace
- cargo build --verbose --all --release --all-features
- cargo test --release --all-features

## Links

- Crate: https://crates.io/crates/oris-runtime
- Docs: https://docs.rs/oris-runtime
- Repo: https://github.com/Colin4k1024/Oris
