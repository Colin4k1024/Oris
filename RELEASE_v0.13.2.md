# v0.13.2 - Remote replay follow-up fixes

`oris-runtime` now ships the remote replay follow-up fixes so cold-start candidate ranking stays stable and Evo asset export no longer rescans the full event log twice.

## What's in this release

- Normalized remote cold-start replay scoring so overlapping signal fragments do not inflate candidate scores above full query coverage.
- Reduced Evo asset export and fetch overhead by reusing a single event scan for projection rebuilding and replay payload packaging.

## Validation

- cargo fmt --all -- --check
- cargo test -p oris-evokernel --lib
- cargo test -p oris-evokernel --test evolution_lifecycle_regression
- ORT_LIB_LOCATION=/Users/jiafan/onnxruntime ORT_PREFER_DYNAMIC_LINK=0 cargo build --verbose --all --release --all-features
- ORT_LIB_LOCATION=/Users/jiafan/onnxruntime ORT_PREFER_DYNAMIC_LINK=0 cargo test --release --all-features

## Links

- Crate: https://crates.io/crates/oris-runtime
- Docs: https://docs.rs/oris-runtime
- Repo: https://github.com/Colin4k1024/Oris
