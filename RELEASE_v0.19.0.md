# v0.19.0 - MCP bootstrap and capability discovery scaffold

`oris-runtime` v0.19.0 introduces a feature-gated MCP bootstrap path and capability discovery mapping for Oris runtime integration.

## What's in this release

- Added `mcp-experimental` feature gating with MCP bootstrap configuration (`ORIS_MCP_BOOTSTRAP_ENABLED`, transport, server metadata) and startup wiring.
- Added MCP discovery endpoints (`/v1/mcp/bootstrap`, `/v1/mcp/capabilities`) with default capability registry mapping (`oris.runtime.jobs.run -> POST /v1/jobs/run`) and disabled-by-default behavior.
- Added runtime tests and starter-axum docs/smoke path for MCP bootstrap and capability discovery.

## Validation

- cargo fmt --all
- cargo test -p oris-runtime --all-features mcp_
- cargo fmt --all -- --check
- cargo test --workspace
- cargo build --verbose --all --release --all-features
- cargo test --release --all-features
- cargo publish -p oris-runtime --all-features --dry-run --registry crates-io
- cargo publish -p oris-runtime --all-features --registry crates-io

## Links

- Crate: https://crates.io/crates/oris-runtime
- Docs: https://docs.rs/oris-runtime
- Repo: https://github.com/Colin4k1024/Oris
