# v0.11.0 - EvoKernel signal extraction and solidification queries

`oris-runtime` now ships a more explicit EvoKernel solidification path with deterministic signal extraction and a direct selector query surface, backed by refreshed `oris-evolution` and `oris-evokernel` dependency publishes so the new APIs resolve correctly from crates.io.

## What's in this release

- Added deterministic EvoKernel signal extraction inputs and outputs, plus a persisted `SignalsExtracted` evolution event so successful captures record their normalized signal set and hash.
- Added a direct `EvoKernel::select_candidates(...)` query path and expanded EvoKernel/runtime regression coverage around signal stability and local candidate lookup.

## Validation

- cargo fmt --all -- --check
- cargo test -p oris-evokernel --test evolution_lifecycle_regression -- --nocapture
- cargo test -p oris-runtime --test evolution_feature_wiring --features full-evolution-experimental
- cargo test --workspace
- /bin/zsh -lc 'unset ORT_LIB_LOCATION ORT_PREFER_DYNAMIC_LINK ORT_LIB_PROFILE; cargo build --verbose --all --release --all-features'
- /bin/zsh -lc 'unset ORT_LIB_LOCATION ORT_PREFER_DYNAMIC_LINK ORT_LIB_PROFILE; cargo test --release --all-features'

## Links

- Crate: https://crates.io/crates/oris-runtime
- Docs: https://docs.rs/oris-runtime
- Repo: https://github.com/Colin4k1024/Oris
