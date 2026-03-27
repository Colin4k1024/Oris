# v1.0 Runtime Hardening Baseline — Verification Report

**Issue:** #411 Runtime hardening baseline complete
**Parent Milestone:** v1.0 Trusted Improvement System (#369)
**Status:** Complete
**Date:** 2026-03-27

## Verification Summary

The runtime hardening baseline for v1.0 builds on the v0.70 milestone work and is verified as complete across all five pillars of deterministic execution core.

## Pillar 1: Deterministic Execution Core

### 1.1 Stable Replay Semantics

**Requirement:** Stable replay semantics

**Evidence:**
- [Replay Lifecycle Invariants](replay-lifecycle-invariants.md) — 10 core invariants (INV-1 through INV-10), determinism guard, 5 lifecycle phases (Recording, Replay, Resume, Verification, Timeline Forking)
- Test coverage: 12 tests covering replay side-effect freedom, state equivalence, checkpoint replay, idempotency
- **Source:** `crates/oris-kernel/src/kernel/replay_cursor.rs`, `crates/oris-kernel/src/kernel/replay_verifier.rs`, `crates/oris-kernel/src/kernel/determinism_guard.rs`
- **PR:** #420 (merged)

### 1.2 Stable Interrupt/Resume Behavior

**Requirement:** Stable interrupt/resume behavior

**Evidence:**
- [Interrupt/Resume Invariants](interrupt-resume-invariants.md) — 7 core invariants (INV-I1 through INV-I7), KernelInterruptStatus and ExecutionSuspensionState state machines
- Test coverage: 22 tests covering interrupt consistency, snapshot saving, LIFO matching, resume idempotency
- **Source:** `crates/oris-kernel/src/kernel/driver.rs`, `crates/oris-kernel/src/kernel/kernel_interrupt.rs`, `crates/oris-kernel/src/kernel/execution_suspension.rs`
- **PR:** #421 (merged)

### 1.3 Stable Finalization Semantics

**Requirement:** Stable finalization semantics

**Evidence:**
- Lease terminal state tests in `crates/oris-execution-runtime/src/lease.rs`
- 5 tests verifying terminal states block execution, transitions are guarded, idempotent cleanup
- Terminal states: Completed, Failed, Expired, Cancelled
- **Source:** `crates/oris-execution-runtime/src/lease.rs`
- **PR:** #422 (merged)

### 1.4 Crash/Restart Trust in Bounded Scenarios

**Requirement:** Crash/restart trust in bounded scenarios

**Evidence:**
- SQLite crash-recovery test suite: 5 tests (events survive reopen, snapshots survive reopen, replay from snapshot after reopen, multiple snapshots latest wins, append continues sequence)
- Postgres parity suite: 5 tests mirroring SQLite suite
- **Source:** `crates/oris-kernel/src/kernel/sqlite_store.rs`, `crates/oris-kernel/src/kernel/postgres_store.rs`
- **PR:** #423 (SQLite), #425 (Postgres parity)

### 1.5 Backend Behavior Trustworthy for Production

**Requirement:** Backend behavior trustworthy enough for production-style operation

**Evidence:**
- WAL mode + NORMAL synchronous for SQLite (crash-safe without full sync overhead)
- Advisory locks per run_id for Postgres (correctness under concurrency)
- EventStore atomic append (all-or-nothing)
- SnapshotStore as optimization only (event log is source of truth)
- DeterminismGuard traps nondeterminism in Replay/Verify modes (clock, RNG, thread spawn)
- **Source:** `crates/oris-kernel/src/kernel/event.rs`, `crates/oris-kernel/src/kernel/snapshot.rs`, `crates/oris-kernel/src/kernel/determinism_guard.rs`

## Test Summary

| Crate | Tests | Status |
|-------|-------|--------|
| oris-kernel | 75 passed | All pass |
| oris-execution-runtime | 53 passed | All pass |
| oris-runtime (lib) | 287 passed | All pass |

## Alignment with v0.70 Milestone

The v1.0 runtime hardening baseline directly inherits all v0.70 deliverables:

| v0.70 Deliverable | v1.0 Status |
|-------------------|-------------|
| Replay lifecycle invariants documented | Maintained |
| Interrupt/resume invariants documented | Maintained |
| Finalization semantics tested | Maintained |
| SQLite crash-recovery suite | Maintained |
| Postgres parity suite | Maintained |
| Scheduler fairness tests | N/A (separate concern) |
| Backpressure tests | N/A (separate concern) |

## Conclusion

The runtime hardening baseline is **complete** for v1.0. All five pillars of deterministic execution core are satisfied with documented invariants, test coverage, and production-appropriate backend behavior.

**Parent Milestone Exit Checklist:**
- [x] Runtime hardening baseline complete (this issue)