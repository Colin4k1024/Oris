# v0.17.0 - Harden `/a2a/fetch` validation determinism

`oris-runtime` v0.17.0 tightens EvoMap-compatible `/a2a/fetch` validation behavior so envelope-type errors return stable machine-readable details.

## What's in this release

- Added deterministic `a2a_error_code=ValidationFailed` details for `gep-a2a` `message_type` mismatches in `/a2a/fetch` compatibility parsing.
- Extended regression coverage to assert both error code and expected/actual message-type payload for invalid fetch envelope requests.

## Validation

- cargo fmt --all -- --check
- cargo test -p oris-runtime --features "execution-server,full-evolution-experimental" evolution_a2a_fetch_validation_errors_include_a2a_error_code_details -- --nocapture
- cargo test --workspace
- cargo build --verbose --all --release --all-features
- cargo test --release --all-features
- cargo publish -p oris-runtime --all-features --dry-run
- cargo publish -p oris-runtime --all-features

## Links

- Crate: https://crates.io/crates/oris-runtime
- Docs: https://docs.rs/oris-runtime
- Repo: https://github.com/Colin4k1024/Oris
