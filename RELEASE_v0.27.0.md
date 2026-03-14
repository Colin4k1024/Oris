# v0.27.0 - Bounded Supervised Devloop Expansion

`oris-runtime` now exposes a bounded supervised DEVLOOP path for small
multi-file docs workflows while keeping failure handling fail-closed and
auditable.

## What's in this release

- Expand supervised DEVLOOP from single-file docs tasks to bounded multi-file
  docs tasks under `docs/` with deterministic file-count limits.
- Keep `reason_code`, `recovery_hint`, and fail-closed rejection semantics
  aligned across API outcomes, evolution events, and runtime facade coverage.
- Update devloop documentation to reflect the new bounded docs-task surface.

## Validation

- cargo fmt --all -- --check
- cargo test -p oris-evokernel --test evolution_lifecycle_regression supervised_devloop_ -- --nocapture
- cargo test -p oris-runtime --test evolution_feature_wiring --features full-evolution-experimental -- --nocapture
- cargo test --workspace
- cargo build --verbose --all --release --all-features
- cargo test --release --all-features
- cargo publish -p oris-runtime --all-features --dry-run
- cargo publish -p oris-runtime --all-features

## Links

- Crate: https://crates.io/crates/oris-runtime
- Docs: https://docs.rs/oris-runtime
- Repo: https://github.com/fanjia1024/oris
