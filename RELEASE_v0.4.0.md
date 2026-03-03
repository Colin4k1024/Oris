# v0.4.0 - Spec-Aware EvoKernel Replay Selection

Minor release adding spec-aware replay narrowing and repository-native spec scaffolding for the `oris-runtime` EvoKernel experimental flow.

## What's in this release

- Adds optional `spec_id` narrowing to EvoKernel replay selection, wiring spec-linked mutations through the evolution projection and exact-match replay path.
- Checks in the `specs/behavior`, `specs/repair`, `specs/optimization`, and `specs/evolution` repository layout and keeps the execution server cancellation guard compiled under all feature combinations.

## Validation

- cargo fmt --all -- --check
- cargo test -p oris-evolution -- --nocapture
- cargo test -p oris-evokernel replay_hit_records_capsule_reused -- --nocapture
- cargo test -p oris-runtime --features full-evolution-experimental --test evolution_feature_wiring full_evolution_experimental_paths_resolve -- --nocapture
- cargo test --workspace
- /bin/zsh -lc "unset ORT_LIB_LOCATION ORT_PREFER_DYNAMIC_LINK ORT_LIB_PROFILE; cargo build --verbose --all --release --all-features"
- /bin/zsh -lc "unset ORT_LIB_LOCATION ORT_PREFER_DYNAMIC_LINK ORT_LIB_PROFILE; cargo test --release --all-features"

## Links

- Crate: https://crates.io/crates/oris-runtime
- Docs: https://docs.rs/oris-runtime
- Repo: https://github.com/Colin4k1024/Oris
