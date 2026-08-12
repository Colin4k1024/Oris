# v0.4.0 - Coding Agent Experience Bridge

This release exposes the complete governed Oris Experience Repository capability set to supported coding agents through a portable MCP integration package.

## What's in this release

- Adds capability-scoped MCP tools, resources, and prompts for read, write, governance, and administration workflows.
- Adds native integration assets for Claude Code, OpenCode, Codex, and Grok, including lifecycle handling and a canonical machine-readable capability manifest.
- Preserves caller sandbox and approval boundaries while failing closed for privileged operations.
- Keeps Google ADK explicitly outside the supported-host contract for this release.
- Pins compatible ONNX Runtime FFI and pgvector releases so clean all-feature builds remain reproducible.

## Validation

- `cargo fmt --all -- --check`
- `cargo test -p oris-experience-repo --release --all-features`
- `cargo build --verbose --all --release --all-features`
- `cargo test --release --all-features`
- `cargo publish -p oris-experience-contract --dry-run --registry crates-io`
- `cargo publish -p oris-experience-repo --dry-run --registry crates-io`

## Links

- Crate: https://crates.io/crates/oris-experience-repo
- Docs: https://docs.rs/oris-experience-repo
- Repo: https://github.com/Colin4k1024/Oris
- Issue: https://github.com/Colin4k1024/Oris/issues/452
