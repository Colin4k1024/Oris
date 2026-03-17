# v0.49.0 - Evolution network security hardening

This release hardens evolution-network capsule ingestion with signed envelopes, per-peer rate limiting, and append-only network audit logs.

## What's in this release

- Added Ed25519 envelope signing helpers, persisted node key generation, and signature verification for secured capsule intake.
- Added per-peer capsule rate limiting, structured ACCEPT/REJECT audit logging, and the optional `network-mtls` feature flag in `oris-evolution-network`.
- Preserved the existing remote capsule promotion flow while adding a secure receiver entrypoint and coverage for tampered, unsigned, and rate-limited traffic.

## Validation

- cargo fmt --all -- --check
- cargo test -p oris-evolution-network
- cargo build --verbose --all --release --all-features
- cargo test --release --all-features
- cargo test -p oris-runtime --features "sqlite-persistence,execution-server" kernel::runtime::api_handlers::tests::security_ -- --nocapture --test-threads=1
- cargo publish -p oris-evolution-network --all-features --dry-run
- cargo publish -p oris-evolution-network --all-features

## Links

- Crate: https://crates.io/crates/oris-evolution-network
- Docs: https://docs.rs/oris-evolution-network
- Repo: https://github.com/Colin4k1024/Oris