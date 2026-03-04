# v0.13.1 - EvoKernel lifecycle replay fixes

`oris-runtime` now ships the EvoKernel lifecycle audit fixes so remote replay assets, replay attribution, and compatibility behavior stay consistent in production paths.

## What's in this release

- Fixed remote Evo asset sharing so exported and fetched promoted assets include the mutation payload required for first local replay.
- Preserved replay compatibility by restoring the legacy `ReplayExecutor` entrypoint while recording explicit replay execution IDs separately in Evo events.

## Validation

- cargo fmt --all -- --check
- ORT_LIB_LOCATION=/Users/jiafan/onnxruntime ORT_PREFER_DYNAMIC_LINK=0 cargo build --verbose --all --release --all-features
- cargo test -p oris-evolution --lib
- cargo test -p oris-evokernel --lib
- cargo test -p oris-evokernel --test evolution_lifecycle_regression
- cargo test -p oris-runtime --lib execution_server::api_handlers::tests::evolution_publish_fetch_and_revoke_routes_work
- ORT_LIB_LOCATION=/Users/jiafan/onnxruntime ORT_PREFER_DYNAMIC_LINK=0 cargo test --release --all-features
- cargo publish -p oris-evolution --dry-run --registry crates-io
- cargo publish -p oris-evolution --registry crates-io
- cargo publish -p oris-evokernel --dry-run --registry crates-io
- cargo publish -p oris-evokernel --registry crates-io
- ORT_LIB_LOCATION=/Users/jiafan/onnxruntime ORT_PREFER_DYNAMIC_LINK=0 cargo publish -p oris-runtime --all-features --dry-run --registry crates-io
- ORT_LIB_LOCATION=/Users/jiafan/onnxruntime ORT_PREFER_DYNAMIC_LINK=0 cargo publish -p oris-runtime --all-features --registry crates-io

## Links

- Crate: https://crates.io/crates/oris-runtime
- Docs: https://docs.rs/oris-runtime
- Repo: https://github.com/Colin4k1024/Oris
