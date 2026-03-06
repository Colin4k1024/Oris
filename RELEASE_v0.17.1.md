# v0.17.1 - RFC roadmap closeout alignment

`oris-runtime` v0.17.1 publishes a docs-and-release alignment update that closes roadmap RFC discussions (#106-#109) with explicit strategy decisions and execution boundaries.

## What's in this release

- Added RFC closeout decisions to `docs/ORIS_2.0_STRATEGY.md` for #106-#109, including delivered `/a2a` outcomes and deferred themes (deterministic scheduler hardening, MCP implementation, and long-horizon ecosystem items).
- Synchronized issue/release state so roadmap RFC closure is explicitly tied to a published runtime version.

## Validation

- cargo fmt --all -- --check
- cargo build --verbose --all --release --all-features
- cargo test --release --all-features
- cargo publish -p oris-runtime --all-features --dry-run --registry crates-io
- cargo publish -p oris-runtime --all-features --registry crates-io

## Links

- Crate: https://crates.io/crates/oris-runtime
- Docs: https://docs.rs/oris-runtime
- Repo: https://github.com/Colin4k1024/Oris
