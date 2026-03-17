# oris-runtime v0.41.0

## Summary

Adds a three-state **circuit breaker** to the worker execution chain (Phase 1 P1-07).

## Changes

### New: `CircuitBreaker` (`oris-execution-runtime::circuit_breaker`)

- `CircuitState` enum — `Closed`, `Open { opened_at }`, `HalfOpen`
- `CircuitBreaker::new(probe_window_secs)` — creates a closed breaker
- `CircuitBreaker::trip()` — forces the breaker to `Open`
- `CircuitBreaker::is_open()` — returns `true` while open; auto-transitions to `HalfOpen` after the probe window elapses
- `CircuitBreaker::record_success()` — resets from `HalfOpen` / `Open` back to `Closed`

### Updated: `WorkerHealthTracker`

- `with_circuit_breaker(Arc<CircuitBreaker>)` builder — wires a shared breaker
- `record_expiry()` now trips the attached breaker when a worker is quarantined
- New `record_success(worker_id)` method — clears quarantine and resets the breaker to `Closed`

### Updated: `SkeletonScheduler`

- `with_circuit_breaker(Arc<CircuitBreaker>)` builder — attaches a breaker to the scheduler
- `dispatch_one` / `dispatch_one_with_context` return `SchedulerDecision::Backpressure` (reason: `"circuit breaker open"`) when the breaker is `Open`

## Validation

- `cargo fmt --all -- --check` ✓
- `cargo test -p oris-execution-runtime` — 27/27 passed ✓
- `cargo build --all --release --all-features` clean ✓
- `cargo publish -p oris-execution-runtime --dry-run` ✓
- `cargo publish -p oris-runtime --all-features --dry-run` ✓

## Crate versions

- `oris-execution-runtime` v0.2.15 → **v0.3.0**
- `oris-runtime` v0.40.0 → **v0.41.0**
