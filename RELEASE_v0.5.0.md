# v0.5.0 - EvoKernel Local Economics Wiring

Minor release adding the first end-to-end EVU economics wiring for the `oris-runtime` EvoKernel experimental flow.

## What's in this release

- Adds EVU stake reservation for remote-facing asset export, so insufficient local balance blocks publish without blocking local replay.
- Rewards or penalizes the recorded remote publisher after replay, and uses reputation as a bounded secondary tie-breaker when replay candidates are otherwise equally valid.

## Validation

- cargo fmt --all -- --check
- cargo test -p oris-economics -- --nocapture
- cargo test -p oris-evokernel -- --nocapture
- cargo test -p oris-runtime --features full-evolution-experimental --test evolution_feature_wiring full_evolution_experimental_paths_resolve -- --nocapture
- cargo test --workspace
- /bin/zsh -lc "unset ORT_LIB_LOCATION ORT_PREFER_DYNAMIC_LINK ORT_LIB_PROFILE; cargo build --verbose --all --release --all-features"
- /bin/zsh -lc "unset ORT_LIB_LOCATION ORT_PREFER_DYNAMIC_LINK ORT_LIB_PROFILE; cargo test --release --all-features"

## Links

- Crate: https://crates.io/crates/oris-runtime
- Docs: https://docs.rs/oris-runtime
- Repo: https://github.com/Colin4k1024/Oris
