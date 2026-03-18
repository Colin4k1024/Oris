# v0.53.0 - Bounded Autonomous PR Lane for Low-Risk Tasks

oris-runtime now exposes a fail-closed autonomous PR lane that prepares deterministic branch and evidence-backed PR artifacts only for explicitly approved low-risk task classes.

## What's in this release

- Added machine-readable autonomous PR lane contracts covering delivery summary, branch name, PR payload, evidence bundle, delivery status, approval state, and stable reason codes.
- Added an EvoKernel autonomous PR lane gate that approves only low-risk docs and lint tasks with passing validation evidence and denies all other cases fail-closed.
- Kept the autonomous PR lane wired through the runtime facade and aligned with orchestrator delivery structures so reviewers get deterministic branch and PR evidence artifacts.

## Validation

- cargo test -p oris-orchestrator autonomous_pr_ -- --nocapture
- cargo test -p oris-runtime --test evolution_feature_wiring --features full-evolution-experimental -- --nocapture
- cargo test -p oris-runtime --test agent_self_evolution_travel_network --features full-evolution-experimental -- --nocapture
- cargo test --workspace
- cargo fmt --all -- --check
- cargo build --verbose --all --release --all-features
- cargo test --release --all-features
- cargo publish -p oris-runtime --all-features --registry crates-io --dry-run
- cargo publish -p oris-runtime --all-features --registry crates-io

## Links

- Crate: https://crates.io/crates/oris-runtime
- Docs: https://docs.rs/oris-runtime
- Repo: https://github.com/Colin4k1024/Oris