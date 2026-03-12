# v0.23.0 - GEP Delta Sync and Resume Token

Add incremental GEP synchronization primitives so peers can pull deltas with resumable cursors and receive machine-readable sync audit evidence.

## What's in this release

- Added `since_cursor` and `resume_token` support for publish/fetch protocol messages, with deterministic cursor progression and resume token validation.
- Added `sync_audit` response evidence (scanned/applied/skipped/failed counts and reasons) and idempotent import behavior across evokernel + runtime compat APIs.
- Extended runtime A2A fetch compatibility APIs and tests to verify delta synchronization and resume-token continuation end-to-end.

## Validation

- cargo fmt --all -- --check
- cargo test -p oris-evolution-network
- cargo test -p oris-evokernel
- cargo test -p oris-runtime evolution_a2a_fetch_returns_sync_cursor_and_supports_resume_token_delta --features "sqlite-persistence,execution-server,agent-contract-experimental,evolution-network-experimental" -- --nocapture --test-threads=1
- cargo test --workspace -- --skip official_experience_reuse_with_real_qwen
- cargo build --verbose --all --release --all-features
- cargo test --release --all-features -- --skip official_experience_reuse_with_real_qwen
- cargo publish -p oris-evolution-network --registry crates-io
- cargo publish -p oris-evokernel --registry crates-io
- cargo publish -p oris-runtime --all-features --dry-run --registry crates-io
- cargo publish -p oris-runtime --all-features --registry crates-io

## Links

- Crate: https://crates.io/crates/oris-runtime
- Docs: https://docs.rs/oris-runtime
- Repo: https://github.com/Colin4k1024/Oris
