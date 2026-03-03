# v0.8.2 - DEVLOOP proposal example wiring

`oris-runtime` now has a checked-in DEVLOOP example that exercises the proposal-driven EvoKernel path through the `full-evolution-experimental` facade wiring.

## What's in this release

- `examples/evo_oris_repo` now uses `oris-runtime` re-exports with `full-evolution-experimental` and runs `AgentTask -> MutationProposal -> capture_from_proposal -> replay_or_fallback`.
- The example demonstrates two agent sources entering the same capture pipeline and replaying on the second pass, while docs snapshots and feature-wiring coverage now reflect that path.

## Validation

- cargo fmt --all -- --check
- cargo test -p oris-runtime --features full-evolution-experimental --test evolution_feature_wiring full_evolution_experimental_paths_resolve -- --nocapture
- cargo run -p evo_oris_repo
- cargo test --workspace
- /bin/zsh -lc "unset ORT_LIB_LOCATION ORT_PREFER_DYNAMIC_LINK ORT_LIB_PROFILE; cargo build --verbose --all --release --all-features"
- /bin/zsh -lc "unset ORT_LIB_LOCATION ORT_PREFER_DYNAMIC_LINK ORT_LIB_PROFILE; cargo test --release --all-features"
- cargo publish -p oris-runtime --all-features --dry-run --registry crates-io
- cargo publish -p oris-runtime --all-features --registry crates-io

## Links

- Crate: https://crates.io/crates/oris-runtime
- Docs: https://docs.rs/oris-runtime
- Repo: https://github.com/Colin4k1024/Oris
