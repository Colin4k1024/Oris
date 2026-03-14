# v0.24.0 - Stable Task-Class Replay Evidence

Strengthen self-evolution replay generalization so semantically equivalent multi-signal tasks reuse learned capsules more reliably while preserving stable machine-readable task-class evidence.

## What's in this release

- Normalized semantically equivalent task signals across evolution and evokernel replay matching, including missing-state aliases and filler-token suppression, so same-class tasks replay consistently without regressing adjacent negative samples.
- Stabilized task-class evidence in replay feedback and derived gene metadata, and added regression coverage to prove multi-signal semantic variants keep replay labels audit-friendly and deterministic.

## Validation

- cargo fmt --all -- --check
- cargo test -p oris-evolution normalized_signal_overlap -- --nocapture
- cargo test -p oris-evokernel --test evolution_lifecycle_regression multi_signal_semantic_variants_keep_task_class_feedback_stable -- --nocapture
- cargo test -p oris-evokernel --test evolution_lifecycle_regression replay_feedback_surfaces_planner_hints_and_reasoning_savings -- --nocapture
- cargo test -p oris-evokernel --lib replay_roi_release_gate_summary_window_boundary_filters_old_events -- --nocapture
- cargo test -p oris-evokernel --release --lib replay_roi_release_gate_summary_window_boundary_filters_old_events -- --nocapture
- cargo test -p oris-evokernel --test evolution_lifecycle_regression -- --nocapture
- cargo test -p oris-runtime --test agent_self_evolution_travel_network --features full-evolution-experimental travel_network_demo_flow_captures_publishes_imports_and_replays -- --nocapture
- cargo test -p oris-runtime --release --test agent_self_evolution_travel_network --features full-evolution-experimental travel_network_demo_flow_captures_publishes_imports_and_replays -- --nocapture
- cargo test --workspace
- cargo build --verbose --all --release --all-features
- cargo test --release --all-features
- cargo publish -p oris-agent-contract --dry-run --allow-dirty --registry crates-io
- cargo publish -p oris-agent-contract --allow-dirty --registry crates-io
- cargo publish -p oris-evolution --dry-run --allow-dirty --registry crates-io
- cargo publish -p oris-evolution --allow-dirty --registry crates-io
- cargo publish -p oris-evokernel --dry-run --allow-dirty --registry crates-io
- cargo publish -p oris-evokernel --allow-dirty --registry crates-io
- cargo publish -p oris-runtime --all-features --dry-run --allow-dirty --registry crates-io
- cargo publish -p oris-runtime --all-features --allow-dirty --registry crates-io

## Links

- Crate: https://crates.io/crates/oris-runtime
- Docs: https://docs.rs/oris-runtime
- Repo: https://github.com/Colin4k1024/Oris
