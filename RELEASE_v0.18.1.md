# v0.18.1 - Deterministic scheduler regression matrix for A2A lease and replay parity

`oris-runtime` v0.18.1 ships a deterministic regression matrix for compatibility `/a2a` scheduler lease semantics and replay-equivalence checks.

## What's in this release

- Added a deterministic matrix test for `/a2a` compatibility flows that asserts replay-equivalent outcomes across:
  - claim conflict under active lease,
  - heartbeat visibility before and after forced lease expiry,
  - reclaim semantics after expiry,
  - completion idempotency on duplicate complete.
- Added explicit non-owner running-report rejection coverage in the matrix via sender-scoped `404` expectations.
- Updated operator docs with a single command to run the deterministic matrix and a triage map for matrix-failure signals.

## Validation

- cargo fmt --all
- cargo test -p oris-runtime --all-features evolution_a2a_scheduler_deterministic_matrix_is_replay_equivalent
- cargo fmt --all -- --check
- cargo build --verbose --all --release --all-features
- cargo test -p oris-runtime --release --all-features evolution_a2a_compat_e2e_fetch_claim_complete_and_heartbeat_supports_route_variants -- --nocapture
- cargo test --release --all-features
- cargo publish -p oris-runtime --all-features --dry-run --registry crates-io
- cargo publish -p oris-runtime --all-features --registry crates-io

## Links

- Crate: https://crates.io/crates/oris-runtime
- Docs: https://docs.rs/oris-runtime
- Repo: https://github.com/Colin4k1024/Oris
