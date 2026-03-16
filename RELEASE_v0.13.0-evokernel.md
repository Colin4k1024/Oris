# Release: oris-evokernel v0.13.0

**Issue**: #283 — EVO26-AUTO Stream D: Confidence Control Daemon

## Summary

Adds `ConfidenceDaemon` — a background `tokio::spawn` task that periodically
revalidates tracked evolution assets and automatically demotes or quarantines
those whose confidence falls below `MIN_REPLAY_CONFIDENCE`.

## Changes

### New: `crates/oris-evokernel/src/confidence_daemon.rs`

- **`ConfidenceDaemonConfig`** — configures `poll_interval` and
  `demotion_confidence_threshold` (defaults to `MIN_REPLAY_CONFIDENCE = 0.35`).
- **`TrackedAsset`** — entry per asset holding `asset_id`, `ConfidenceState`,
  `failure_count`, `decayed_confidence`, `replay_eligible`.
- **`ConfidenceEvaluator`** — `Send + Sync` trait decoupling the daemon from
  `EvoKernel<S>`; implemented by `EvoKernel` and test doubles.
- **`ConfidenceDaemon`** — main struct with:
  - `track()` — register or update an asset.
  - `snapshot()` — read current state of all assets.
  - `run_cycle()` — synchronous revalidation sweep (evaluates every non-quarantined
    asset, calls demotion for failures, blocks quarantined assets from replay).
  - `spawn()` → `JoinHandle<()>` — launches the async periodic loop.

### Modified: `crates/oris-evokernel/src/lib.rs`

- Added `pub mod confidence_daemon;`.

### Modified: `crates/oris-runtime/Cargo.toml`

- Updated `oris-evokernel` version constraint: `0.12.6` → `0.13.0`.

## Version Bump

`oris-evokernel`: `0.12.6` → `0.13.0` (minor — new public module).

## Tests Added (6 tests in `confidence_daemon::tests`)

- `confidence_daemon_healthy_asset_stays_eligible`
- `confidence_daemon_below_threshold_triggers_demotion`
- `confidence_daemon_quarantine_auto_transition`
- `confidence_daemon_quarantined_excluded_from_replay`
- `confidence_daemon_spawn_returns_join_handle`
- `confidence_daemon_multiple_assets_independent`
