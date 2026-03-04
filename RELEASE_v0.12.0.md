# v0.12.0 - EvoKernel multi-agent coordination

`oris-runtime` now ships opt-in multi-agent coordination contracts and a deterministic in-memory EvoKernel coordinator for planner, coder, repair, and optimizer workflows.

## What's in this release

- Added multi-agent coordination DTOs to the agent contract surface: roles, coordination primitives, tasks, messages, plans, and results.
- Added `MultiAgentCoordinator` plus `EvoKernel::coordinate(...)`, with deterministic sequential, parallel, and conditional scheduling plus retry-aware failure handling.
- Expanded EvoKernel/runtime regression coverage for planner-to-coder handoffs, repair-after-failure, optimizer gating, parallel merge ordering, retries, and conditional skips.

## Validation

- cargo fmt --all -- --check
- cargo test -p oris-evokernel coordination -- --nocapture
- cargo test -p oris-runtime --test evolution_feature_wiring --features full-evolution-experimental
- cargo test --workspace
- env -u ORT_LIB_LOCATION -u ORT_PREFER_DYNAMIC_LINK -u ORT_LIB_PROFILE cargo build --verbose --all --release --all-features
- env -u ORT_LIB_LOCATION -u ORT_PREFER_DYNAMIC_LINK -u ORT_LIB_PROFILE cargo test --release --all-features

## Links

- Crate: https://crates.io/crates/oris-runtime
- Docs: https://docs.rs/oris-runtime
- Repo: https://github.com/Colin4k1024/Oris
