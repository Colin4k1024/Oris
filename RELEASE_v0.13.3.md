# v0.13.3 - Evo consistency hardening

`oris-runtime` now ships a tighter Evo consistency pass so projection reads, remote replay settlement, and remote asset imports all stay aligned under replay and synchronization.

## What's in this release

- Fixed remote replay publisher attribution so reputation bias and EVU settlement follow the actual capsule selected for replay, even when multiple remote capsules share a gene.
- Hardened Evo projection reads and remote import behavior so selector, replay, fetch, metrics, and repeated remote syncs all observe the same store snapshot contract without duplicate downgrade writes.

## Validation

- cargo fmt --all -- --check
- cargo test -p oris-evolution --lib
- cargo test -p oris-evokernel --lib
- cargo test -p oris-evokernel --test evolution_lifecycle_regression
- ORT_LIB_LOCATION=/Users/jiafan/onnxruntime ORT_PREFER_DYNAMIC_LINK=0 cargo build --verbose --all --release --all-features
- ORT_LIB_LOCATION=/Users/jiafan/onnxruntime ORT_PREFER_DYNAMIC_LINK=0 cargo test --release --all-features
- cargo publish -p oris-evolution --dry-run --registry crates-io
- cargo publish -p oris-evolution --registry crates-io
- cargo publish -p oris-evokernel --dry-run --registry crates-io
- cargo publish -p oris-evokernel --registry crates-io
- unset HTTP_PROXY HTTPS_PROXY ALL_PROXY http_proxy https_proxy all_proxy; export CARGO_HTTP_PROXY=; export ORT_LIB_LOCATION=/Users/jiafan/onnxruntime; export ORT_PREFER_DYNAMIC_LINK=0; cargo publish -p oris-runtime --all-features --dry-run --registry crates-io
- unset HTTP_PROXY HTTPS_PROXY ALL_PROXY http_proxy https_proxy all_proxy; export CARGO_HTTP_PROXY=; export ORT_LIB_LOCATION=/Users/jiafan/onnxruntime; export ORT_PREFER_DYNAMIC_LINK=0; cargo publish -p oris-runtime --all-features --registry crates-io

## Links

- Crate: https://crates.io/crates/oris-runtime
- Docs: https://docs.rs/oris-runtime
- Repo: https://github.com/Colin4k1024/Oris
