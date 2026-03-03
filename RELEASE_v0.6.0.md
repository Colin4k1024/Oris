# v0.6.0 - Environment-Aware Replay Ranking

Minor release adding environment-aware replay ranking for the EvoKernel experimental evolution flow in `oris-runtime`.

## What's in this release

- Selector scoring now weights environment similarity, so replay candidates are ranked by how closely their recorded execution environment matches the current run.
- Replay now prefers the closest matching Capsule within matching assets, improving reuse accuracy when multiple proven solutions share the same signals.

## Validation

- cargo fmt --all -- --check
- cargo test -p oris-evolution -- --nocapture
- cargo test -p oris-evokernel remote_replay_prefers_closest_environment_match -- --nocapture
- cargo test -p oris-evokernel replay_hit_records_capsule_reused -- --nocapture
- cargo test -p oris-runtime --features full-evolution-experimental --test evolution_feature_wiring full_evolution_experimental_paths_resolve -- --nocapture
- cargo test --workspace
- /bin/zsh -lc "unset ORT_LIB_LOCATION ORT_PREFER_DYNAMIC_LINK ORT_LIB_PROFILE; cargo build --verbose --all --release --all-features"
- /bin/zsh -lc "unset ORT_LIB_LOCATION ORT_PREFER_DYNAMIC_LINK ORT_LIB_PROFILE; cargo test --release --all-features"

## Links

- Crate: https://crates.io/crates/oris-runtime
- Docs: https://docs.rs/oris-runtime
- Repo: https://github.com/Colin4k1024/Oris
