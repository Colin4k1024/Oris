# v0.20.0 - Stable /a2a production boundary

`oris-runtime` v0.20.0 stabilizes the production `/a2a/*` compatibility surface while keeping broader evolution-network routes behind explicit experimental gates.

## What's in this release

- Added the `a2a-production` feature to expose stable `/a2a/*` compatibility routes for production workflows.
- Kept evolution-network publish/fetch/revoke and legacy `/evolution/a2a/*` surfaces behind experimental feature gates unless explicitly enabled.
- Added route-boundary regression coverage and updated migration/runbook documentation for stable versus experimental runtime behavior.

## Validation

- cargo fmt --all -- --check
- cargo test -p oris-runtime --features "execution-server,sqlite-persistence,a2a-production" a2a_production_route_boundary_hides_evolution_network_routes -- --nocapture
- cargo test --workspace
- cargo build --verbose --all --release --all-features
- cargo test --release --all-features
- cargo publish -p oris-runtime --all-features --dry-run --registry crates-io
- cargo publish -p oris-runtime --all-features --registry crates-io

## Links

- Crate: https://crates.io/crates/oris-runtime
- Docs: https://docs.rs/oris-runtime
- Repo: https://github.com/Colin4k1024/Oris
