# v0.28.0 - Federated Revocation Hardening

`oris-runtime` now fail-closes spoofed remote revoke requests and preserves remote attribution through replay revocation evidence.

## What's in this release

- Hardened federated revoke handling so imported remote assets can only be revoked by the sender that originally published them, while mixed-ownership revoke requests are rejected as a whole.
- Added stable remote attribution evidence for replay-failure revocations and locked the import, replay, and revoke path with evokernel and travel-network regressions.

## Validation

- cargo fmt --all -- --check
- cargo test -p oris-evokernel --test evolution_lifecycle_regression remote_revoke_ -- --nocapture
- cargo test -p oris-evokernel --test evolution_lifecycle_regression remote_replay_failure_ -- --nocapture
- cargo test -p oris-evokernel --lib
- cargo test -p oris-runtime --test agent_self_evolution_travel_network --features full-evolution-experimental -- --nocapture
- cargo test --workspace
- cargo build --verbose --all --release --all-features
- cargo test --release --all-features
- cargo publish -p oris-evokernel --dry-run --registry crates-io
- cargo publish -p oris-runtime --all-features --dry-run --registry crates-io
- cargo publish -p oris-runtime --all-features --registry crates-io

## Links

- Crate: https://crates.io/crates/oris-runtime
- Docs: https://docs.rs/oris-runtime
- Repo: https://github.com/fanjia1024/oris
