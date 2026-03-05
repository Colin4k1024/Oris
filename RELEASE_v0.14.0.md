# v0.14.0 - EvoKernel staged self-evolution hardening

`oris-runtime` now ships the staged EVO-01 through EVO-05 self-evolution hardening scope on `main`, including broader deterministic replay matching, confidence lifecycle controls, replay feedback surfaces, and supervised DEVLOOP boundaries.

## What's in this release

- Expanded deterministic task-class replay matching and strengthened negative controls for unrelated task classes.
- Added continuous confidence lifecycle behavior, including confidence decay and revalidation-driven replay eligibility checks.
- Exposed agent-facing replay feedback with planner directives, fallback reasons, and reasoning-avoidance metrics.
- Added bounded supervised DEVLOOP policy coverage and shipped the staged self-evolution acceptance checklist updates through federated hardening.

## Validation

- cargo fmt --all -- --check
- cargo test -p oris-evokernel --test evolution_lifecycle_regression -- --nocapture
- cargo test -p oris-evolution --lib
- cargo test -p oris-runtime --test evolution_feature_wiring --features full-evolution-experimental
- cargo build --verbose --all --release --all-features
- cargo test --release --all-features
- cargo publish -p oris-runtime --all-features --dry-run
- cargo publish -p oris-runtime --all-features

## Links

- Crate: https://crates.io/crates/oris-runtime
- Docs: https://docs.rs/oris-runtime
- Repo: https://github.com/Colin4k1024/Oris
