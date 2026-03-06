# v0.15.0 - Add EvoMap `/a2a/*` Compatibility Facade Routes

This release adds EvoMap-style `/a2a/*` namespace facade routes to `oris-runtime` while preserving existing `/v1/evolution/a2a/*` and `/evolution/a2a/*` behavior.

## What's in this release

- Added `/a2a/hello`, `/a2a/tasks/distribute`, `/a2a/tasks/claim`, and `/a2a/tasks/report` routes that map to existing A2A compatibility handlers.
- Added route-contract regression coverage to verify new facade routing and existing compatibility flows remain stable.
- Added feature-gate boundary coverage to ensure `/a2a/*` routes remain unavailable when `evolution-network-experimental` is disabled.

## Validation

- cargo fmt --all -- --check
- cargo test -p oris-runtime --features execution-server evolution_a2a_namespace_facade_routes_remain_feature_gated_when_disabled -- --nocapture
- cargo test -p oris-runtime --features "execution-server,full-evolution-experimental" evolution_a2a_namespace_facade_alias_routes_map_to_existing_compat_handlers -- --nocapture
- cargo test -p oris-runtime --features "execution-server,full-evolution-experimental" evolution_a2a_compat_distribute_and_report_map_to_session_flow -- --nocapture
- cargo test --workspace
- cargo build --verbose --all --release --all-features
- cargo test --release --all-features

## Links

- Crate: https://crates.io/crates/oris-runtime
- Docs: https://docs.rs/oris-runtime
- Repo: https://github.com/Colin4k1024/Oris
