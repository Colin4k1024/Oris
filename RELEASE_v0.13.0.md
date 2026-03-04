# v0.13.0 - EvoKernel bootstrap and initial seeding

`oris-runtime` now ships an explicit, deterministic EvoKernel bootstrap API that can seed an empty evolution store with quarantined starter capsules.

## What's in this release

- Added `SeedTemplate`, `BootstrapReport`, and `EvoKernel::bootstrap_if_empty(...)` for opt-in initial seeding of empty evolution stores.
- Added a built-in four-template bootstrap catalog with append-only seed events, deterministic IDs, and quarantined seed capsules that stay out of replay until later local validation.
- Expanded EvoKernel/runtime regression coverage for bootstrap counts, quarantine state, idempotence, append-only history, and seed discoverability through `select_candidates(...)`.

## Validation

- cargo fmt --all -- --check
- cargo test -p oris-evokernel bootstrap -- --nocapture
- cargo test -p oris-runtime --test evolution_feature_wiring --features full-evolution-experimental
- cargo test --workspace
- env -u ORT_LIB_LOCATION -u ORT_PREFER_DYNAMIC_LINK -u ORT_LIB_PROFILE cargo build --verbose --all --release --all-features
- env -u ORT_LIB_LOCATION -u ORT_PREFER_DYNAMIC_LINK -u ORT_LIB_PROFILE cargo test --release --all-features

## Links

- Crate: https://crates.io/crates/oris-runtime
- Docs: https://docs.rs/oris-runtime
- Repo: https://github.com/Colin4k1024/Oris
