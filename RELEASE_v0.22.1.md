# v0.22.1 - GEP Compatibility Matrix Hardening

Strengthen GEP envelope/schema compatibility validation so protocol mismatch and payload errors return deterministic A2A-compatible error details.

## What's in this release

- Added deterministic `a2a_error_code` details for GEP envelope and hello parsing failures (protocol, version, message type, sender, payload).
- Expanded GEP compliance tests to lock schema/version/envelope/message_type behavior and fallback translation paths against regressions.

## Validation

- cargo fmt --all -- --check
- cargo test -p oris-runtime --features "full-evolution-experimental execution-server sqlite-persistence" execution_server::api_handlers::tests:: -- --nocapture
- cargo test -p oris-evolution --lib
- cargo build --verbose --all --release --all-features
- cargo test --release --all-features
- cargo publish -p oris-execution-runtime --dry-run --registry crates-io
- cargo publish -p oris-execution-runtime --registry crates-io
- cargo publish -p oris-runtime --all-features --dry-run --registry crates-io
- cargo publish -p oris-runtime --all-features --registry crates-io

## Links

- Crate: https://crates.io/crates/oris-runtime
- Docs: https://docs.rs/oris-runtime
- Repo: https://github.com/Colin4k1024/Oris
