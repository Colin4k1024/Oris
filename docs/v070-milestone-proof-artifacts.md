# v0.70 Runtime Hardening — Milestone Proof Artifacts

**Milestone:** v0.70 Runtime Hardening ([#363](https://github.com/Colin4k1024/Oris/issues/363))
**Status:** Complete
**Date:** 2026-03-26

## Deliverables

### 1. Runtime Invariants Documentation

| Document | PR | Content |
|----------|----|---------|
| [Replay Lifecycle Invariants](replay-lifecycle-invariants.md) | [#420](https://github.com/Colin4k1024/Oris/pull/420) | 10 core invariants (INV-1 through INV-10), determinism guard, 5 lifecycle phases, full test coverage mapping |
| [Interrupt/Resume Invariants](interrupt-resume-invariants.md) | [#421](https://github.com/Colin4k1024/Oris/pull/421) | 7 core invariants (INV-I1 through INV-I7), KernelInterruptStatus and ExecutionSuspensionState state machines, 22 tests mapped |

### 2. Crash-Recovery Test Report

| Suite | PR | Tests | Result |
|-------|----|-------|--------|
| SQLite crash-recovery | [#423](https://github.com/Colin4k1024/Oris/pull/423) | 5 tests: events survive reopen, snapshots survive reopen, replay from snapshot after reopen, multiple snapshots latest wins, append continues sequence | All pass |

### 3. Backend Parity Test Report

| Suite | PR | Tests | Result |
|-------|----|-------|--------|
| Postgres parity | [#425](https://github.com/Colin4k1024/Oris/pull/425) | 5 tests mirroring SQLite suite: events survive reconnect, snapshots survive reconnect, replay from snapshot, latest snapshot wins, sequence continuation | All pass (env-gated) |

### 4. Finalization Semantics Test Report

| Suite | PR | Tests | Result |
|-------|----|-------|--------|
| Lease finalization | [#422](https://github.com/Colin4k1024/Oris/pull/422) | 5 tests: all terminal states block execution, terminal rejects further transitions, active accepts all transitions, idempotent double-terminal rejected, active state verification | All pass |

### 5. Fairness/Backpressure Stress Report

| Suite | PR | Tests | Result |
|-------|----|-------|--------|
| Scheduler fairness | [#424](https://github.com/Colin4k1024/Oris/pull/424) | 7 tests: FCFS ordering, PriorityWeighted dispatch, conflict skip, context override, RoundRobin fallback, multi-worker independence, empty candidates | All pass |
| Backpressure | [#424](https://github.com/Colin4k1024/Oris/pull/424) | 7 tests: per-tenant throttle, per-worker decrement, queue depth boundary, below-limit dispatch, circuit breaker, independent tenant limits, metrics tracking | All pass |

### 6. Storage Lifecycle Schema Note

- **SQLite:** WAL mode, NORMAL synchronous. Tables: `kernel_events (run_id, seq, event_json, created_at_ms)`, `kernel_snapshots (run_id, at_seq, state_json, created_at_ms)`. Upsert via `ON CONFLICT DO UPDATE`.
- **Postgres:** JSONB columns, advisory locks per run_id. Tables: `kernel_events (run_id, seq, event_json JSONB, created_at TIMESTAMPTZ)`, `kernel_snapshots (run_id, at_seq, state_json JSONB, created_at TIMESTAMPTZ)`. Schema-isolated test runs.
- **Parity:** Both backends implement identical `EventStore` and `SnapshotStore<S>` trait contracts. Semantic behavior verified equivalent via mirrored test suites.

## Summary

All v0.70 sub-issues resolved:
- [x] #370 Replay lifecycle invariants documented
- [x] #371 Interrupt/resume invariants documented
- [x] #372 Finalization semantics tested
- [x] #373 SQLite crash-recovery suite passes
- [x] #374 Postgres parity suite passes
- [x] #375 Scheduler fairness tests added
- [x] #376 Backpressure tests added
- [x] #377 Milestone proof artifacts captured (this document)
