# v0.9.0 - OEN quarantine release path

`oris-runtime` now completes the experimental OEN import workflow by keeping remote capsules quarantined until a local replay validation succeeds and then releasing them for normal reuse.

## What's in this release

- Remote OEN imports no longer trust remote lifecycle events to promote capsules immediately; imported capsules stay quarantined until Oris validates them locally.
- The experimental OEN replay path can now cold-start from quarantined remote capsules and promotes them only after the first successful local replay validation.

## Validation

- cargo fmt --all -- --check
- cargo test -p oris-evokernel remote_ -- --nocapture
- cargo test -p oris-runtime evolution_publish_fetch_and_revoke_routes_work --features "execution-server,evolution-network-experimental" -- --nocapture
- cargo test --workspace
- cargo build --verbose --all --release --all-features
- cargo test --release --all --all-features
- CARGO_HOME=/tmp/oris-cargo-home-release cargo publish -p oris-evokernel --dry-run --registry crates-io
- CARGO_HOME=/tmp/oris-cargo-home-release cargo publish -p oris-evokernel --registry crates-io
- HOME=/tmp/oris-publish-home CARGO_HOME=/tmp/oris-publish-home/.cargo RUSTUP_HOME=/Users/jiafan/.rustup RUSTUP_TOOLCHAIN=stable-aarch64-apple-darwin cargo publish -p oris-runtime --all-features --dry-run --registry crates-io
- HOME=/tmp/oris-publish-home CARGO_HOME=/tmp/oris-publish-home/.cargo RUSTUP_HOME=/Users/jiafan/.rustup RUSTUP_TOOLCHAIN=stable-aarch64-apple-darwin cargo publish -p oris-runtime --all-features --registry crates-io

## Links

- Crate: https://crates.io/crates/oris-runtime
- Docs: https://docs.rs/oris-runtime
- Repo: https://github.com/Colin4k1024/Oris
