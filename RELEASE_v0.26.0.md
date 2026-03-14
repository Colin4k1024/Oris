# v0.26.0 - Replay ROI Stability

Stabilize replay ROI metrics so runtime release-gate evidence stays comparable to
metrics snapshots across the same replay history.

## What's in this release

- Unified evokernel replay ROI aggregation so `metrics_snapshot()` and replay
  release-gate summaries consume the same task-class and source totals.
- Preserved legacy fallback reconstruction for histories that predate
  `ReplayEconomicsRecorded`, preventing release-gate summaries from drifting to
  zero while metrics still report replay activity.
- Tightened runtime travel-network regression coverage so release-gate contract
  input must match the generated replay ROI summary for the same window.

## Validation

- cargo fmt --all -- --check
- cargo test -p oris-evokernel --lib replay_roi_release_gate_summary_ -- --nocapture
- cargo test -p oris-runtime --test agent_self_evolution_travel_network --features full-evolution-experimental -- --nocapture
- cargo test --workspace
- cargo build --verbose --all --release --all-features
- cargo test --release --all-features
- cargo publish -p oris-evokernel --registry crates-io
- cargo publish -p oris-runtime --all-features --dry-run --registry crates-io
- cargo publish -p oris-runtime --all-features --registry crates-io

## Links

- Crate: https://crates.io/crates/oris-runtime
- Docs: https://docs.rs/oris-runtime
- Repo: https://github.com/Colin4k1024/Oris
