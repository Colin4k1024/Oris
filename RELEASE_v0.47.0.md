# v0.47.0 - Evolution Scenario Example Suite (P2-07)

## Summary

Adds runnable end-to-end demos under `examples/evo_oris_repo/` so contributors
can exercise webhook intake, confidence lifecycle transitions, and cross-node
capsule exchange without assembling a custom local setup.

## Changes

### `examples/evo_oris_repo`

- Added `intake_webhook_demo` to simulate a GitHub Actions failure webhook,
  emit an `IntakeEvent`, and show the detect-stage signal handoff.
- Added `confidence_lifecycle_demo` to print a readable confidence curve across
  successful runs, failures, time decay, re-evolution threshold crossing, and
  replacement gene promotion.
- Completed `network_exchange` so the output explicitly demonstrates remote
  capsule transfer and replay-based reuse on the receiving node.
- Updated the example package README and binary declarations for the new demos.

## Validation

- `cargo fmt --all -- --check`
- `cargo build -p evo_oris_repo --all-features`
- `cargo run -p evo_oris_repo --bin intake_webhook_demo --all-features`
- `cargo run -p evo_oris_repo --bin confidence_lifecycle_demo --all-features`
- `cargo run -p evo_oris_repo --bin network_exchange --all-features`

## Notes

- No crates.io release was required for this issue because the shipped changes
  are limited to the workspace example package `evo_oris_repo` (`publish = false`).

## Resolves

- Closes #302