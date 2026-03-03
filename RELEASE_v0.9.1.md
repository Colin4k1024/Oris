# v0.9.1 - EvoKernel regression suite expansion

`oris-runtime` now ships a broader external EvoKernel regression suite so replay, sandbox, and governor behavior are exercised through explicit black-box tests.

## What's in this release

- Added dedicated external regression coverage for replay determinism, sandbox boundary enforcement, governor blast-radius gating, and replay-failure revocation.
- Kept the existing end-to-end replay lifecycle path in the same external suite so the full capture-to-reuse flow remains locked by an integration test.

## Validation

- cargo fmt --all -- --check
- cargo test -p oris-evokernel --test evolution_lifecycle_regression -- --nocapture
- cargo test --workspace
- cargo build --verbose --all --release --all-features
- cargo test --release --all-features
- CARGO_HOME=/tmp/oris-cargo-home-release cargo publish -p oris-evokernel --dry-run --registry crates-io
- CARGO_HOME=/tmp/oris-cargo-home-release cargo publish -p oris-evokernel --registry crates-io
- HOME=/tmp/oris-publish-home CARGO_HOME=/tmp/oris-publish-home/.cargo RUSTUP_HOME=/Users/jiafan/.rustup RUSTUP_TOOLCHAIN=stable-aarch64-apple-darwin cargo publish --manifest-path /Users/jiafan/Desktop/poc/Oris/crates/oris-runtime/Cargo.toml -p oris-runtime --all-features --dry-run --registry crates-io
- HOME=/tmp/oris-publish-home CARGO_HOME=/tmp/oris-publish-home/.cargo RUSTUP_HOME=/Users/jiafan/.rustup RUSTUP_TOOLCHAIN=stable-aarch64-apple-darwin cargo publish --manifest-path /Users/jiafan/Desktop/poc/Oris/crates/oris-runtime/Cargo.toml -p oris-runtime --all-features --registry crates-io

## Links

- Crate: https://crates.io/crates/oris-runtime
- Docs: https://docs.rs/oris-runtime
- Repo: https://github.com/Colin4k1024/Oris
