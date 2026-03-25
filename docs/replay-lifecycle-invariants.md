# Replay Lifecycle Invariants

## Overview

The `oris-kernel` crate provides deterministic replay via event sourcing. All runtime state is reconstructable from the event log — snapshots exist only as a performance optimization. This document enumerates every invariant that the replay subsystem upholds, the kernel modes that enforce them, the lifecycle phases they govern, and the tests that verify them.

## Core Invariants

### INV-1: Event Log Is the Single Source of Truth

State is always derived by reducing events through a `Reducer`. Snapshots (`SnapshotStore`) are a cache — never authoritative. Deleting every snapshot and replaying from seq 1 must reproduce the identical state.

**Source:** `crates/oris-kernel/src/kernel/event.rs`, `snapshot.rs`

### INV-2: Replay Never Executes Actions

During replay the `ActionExecutor` is never invoked. Action outcomes (`ActionSucceeded` / `ActionFailed`) are read from the stored event log. `ReplayCursor` is a standalone replay engine with no executor and no step function — live tool execution is hard-disabled.

**Source:** `crates/oris-kernel/src/kernel/replay_cursor.rs`, `driver.rs` (`Kernel::replay()`)

### INV-3: Reducer Must Be Deterministic

The `Reducer<S>` trait contract requires `apply(state, event) -> state` to be a pure function. Given the same event sequence the same state must result, regardless of wall-clock time, randomness, or environment.

**Source:** `crates/oris-kernel/src/kernel/reducer.rs`

### INV-4: Snapshot `at_seq` Invariant

Every `Snapshot<S>` carries an `at_seq` field equal to the sequence number of the last event applied to produce that snapshot's state. Recovery replays only events with `seq > snapshot.at_seq`.

**Source:** `crates/oris-kernel/src/kernel/snapshot.rs`

### INV-5: Event Append Atomicity

`EventStore::append(run_id, events)` is all-or-nothing — all events in a batch succeed or none do. Partial writes never occur.

**Source:** `crates/oris-kernel/src/kernel/event.rs` (trait contract)

### INV-6: Event Ordering

`EventStore::scan(run_id, from_seq)` always returns events in ascending `seq` order.

**Source:** `crates/oris-kernel/src/kernel/event.rs` (trait contract)

### INV-7: Action Pairing

Every `ActionRequested` event is paired with exactly one terminal event — either `ActionSucceeded` or `ActionFailed`. Retries do **not** duplicate `ActionRequested`; the driver retries the executor and only records the final outcome.

**Source:** `crates/oris-kernel/src/kernel/driver.rs`

### INV-8: Interrupt Consistency

Every `Interrupted` event must be matched by a corresponding `Resumed` event. Matching follows LIFO (stack-based) order. `ReplayVerifier::verify()` checks this via `verify_interrupt_consistency`.

**Source:** `crates/oris-kernel/src/kernel/replay_verifier.rs`

### INV-9: Resume Idempotency

Resuming from the same event log with the same `ResumeDecision` yields identical state and event count. `ReplayResume::verify_idempotent()` confirms this by executing two independent resumes and comparing results.

**Source:** `crates/oris-kernel/src/kernel/replay_resume.rs`

### INV-10: Event Stream Hash Determinism

The same event sequence always produces the same SHA-256 hash via canonical JSON serialization. `event_stream_hash()` and `verify_event_stream_hash()` enforce this.

**Source:** `crates/oris-kernel/src/kernel/determinism_guard.rs`

## Determinism Guard

`DeterminismGuard` enforces determinism in **Replay** and **Verify** kernel modes by trapping nondeterministic operations:

| Operation | Method | Normal/Record | Replay/Verify |
|-----------|--------|---------------|---------------|
| Wall-clock read | `check_clock_access()` | Allowed | **Error** |
| Hardware RNG | `check_random_access()` | Allowed | **Error** |
| Thread spawn | `check_spawn_access()` | Allowed | **Error** |

The guard is keyed on `KernelMode` (`Normal`, `Record`, `Replay`, `Verify`). Only `Replay` and `Verify` return `true` from `traps_nondeterminism()`.

**Source:** `crates/oris-kernel/src/kernel/determinism_guard.rs`, `kernel_mode.rs`

## Replay Lifecycle Phases

### Phase A — Recording (Normal / Record Mode)

1. `Kernel::run_until_blocked()` or `resume()` is called.
2. The driver internally calls `restore_state()` — loads the latest snapshot (if any) and replays tail events to reconstruct current state.
3. Each execution step produces events: `StateUpdated`, `ActionRequested` → `ActionSucceeded`/`ActionFailed`, `Interrupted`, `Completed`.
4. Events are atomically appended to the `EventStore`. Snapshots are optionally saved after each event.

### Phase B — Replay

1. `Kernel::replay()` or `ReplayCursor::replay()` scans all events from seq 1 (or from `snapshot.at_seq + 1`).
2. Each event is applied to state via the `Reducer`.
3. No `ActionExecutor` is called — action results come from the log (INV-2).
4. `DeterminismGuard` is active — clock, randomness, and thread spawning are trapped.
5. The result is the fully reconstructed state.

### Phase C — Resume (After Interrupt)

1. `Kernel::resume()` appends `Event::Resumed { value }` to the event store.
2. `run_loop()` calls `restore_state()` which replays all events including the new `Resumed`.
3. Execution continues from the restored state.
4. `ReplayResume` provides a standalone resume path with built-in idempotency verification (INV-9).

### Phase D — Verification

`ReplayVerifier::verify()` runs up to three configurable checks:

| Check | Config field | Failure |
|-------|-------------|---------|
| State hash equality | `verify_state_hash` | `StateHashMismatch` |
| Tool checksum | `verify_tool_checksum` | `ToolChecksumMismatch` |
| Interrupt consistency | `verify_interrupt_consistency` | `UnmatchedInterrupt` / `UnmatchedResume` |

### Phase E — Timeline Forking

1. `TimelineForker::fork()` replays the source run up to `fork_at_seq`, injects an alternate event, then continues replaying remaining events under a new `branch_id`.
2. `clone_timeline()` replays the entire source run under a new `branch_id` without modification.

**Source:** `crates/oris-kernel/src/kernel/timeline_fork.rs`

## Code References

| File | Purpose |
|------|---------|
| `crates/oris-kernel/src/kernel/replay_cursor.rs` | Standalone replay engine (no executor) |
| `crates/oris-kernel/src/kernel/replay_resume.rs` | Resume with idempotency verification |
| `crates/oris-kernel/src/kernel/replay_verifier.rs` | Cryptographic integrity & consistency checks |
| `crates/oris-kernel/src/kernel/determinism_guard.rs` | Nondeterminism traps + event stream hashing |
| `crates/oris-kernel/src/kernel/driver.rs` | `Kernel::replay()`, `replay_from_snapshot()`, `restore_state()` |
| `crates/oris-kernel/src/kernel/timeline_fork.rs` | Timeline forking and cloning |
| `crates/oris-kernel/src/kernel/event.rs` | `Event` enum and `EventStore` trait |
| `crates/oris-kernel/src/kernel/snapshot.rs` | `Snapshot<S>` and `SnapshotStore<S>` trait |
| `crates/oris-kernel/src/kernel/reducer.rs` | `Reducer<S>` trait |
| `crates/oris-kernel/src/kernel/kernel_mode.rs` | `KernelMode` enum |
| `crates/oris-kernel/src/kernel/execution_log.rs` | `ExecutionLog` with per-event state hashes |

## Test Coverage

Tests are grouped by the invariant they verify:

| Invariant | Test | File |
|-----------|------|------|
| INV-1, INV-2 | `replay_no_side_effects` | `driver.rs` |
| INV-2 | `replay_state_equivalence` | `driver.rs` |
| INV-3 | `replay_from_scratch_reconstructs_state` | `replay_cursor.rs` |
| INV-4 | `replay_from_checkpoint_applies_only_tail` | `replay_cursor.rs` |
| INV-4 | `replay_from_snapshot_applies_tail_only` | `driver.rs` |
| INV-4 | `run_loop_replays_only_tail_after_loading_latest_snapshot` | `driver.rs` |
| INV-5, INV-6 | *(enforced by `EventStore` trait contract)* | `event.rs` |
| INV-7 | `retry_then_success_has_single_terminal_success_event` | `driver.rs` |
| INV-7 | `retry_exhausted_has_single_terminal_failed_event` | `driver.rs` |
| INV-7 | `action_result_failure_returns_failed_and_single_terminal_event` | `driver.rs` |
| INV-8 | `verify_interrupt_consistency_unmatched_interrupt` | `replay_verifier.rs` |
| INV-8 | `verify_interrupt_consistency_ok` | `replay_verifier.rs` |
| INV-9 | `resume_idempotent_twice` | `replay_resume.rs` |
| INV-9 | `verify_idempotent_returns_true` | `replay_resume.rs` |
| INV-10 | `event_stream_hash_deterministic` | `determinism_guard.rs` |
| INV-10 | `verify_event_stream_hash_mismatch_fails` | `determinism_guard.rs` |
| Guard | `guard_replay_traps_clock` | `determinism_guard.rs` |
| Guard | `guard_verify_traps_spawn` | `determinism_guard.rs` |
| Guard | `kernel_replay_mode_determinism_guard_traps_clock` | `driver.rs` |
| Replay step | `replay_step_yields_state_after_each_event` | `replay_cursor.rs` |
| Resume | `resume_injects_decision` | `replay_resume.rs` |
| Resume | `run_until_blocked_then_resume` | `driver.rs` |
| Timeline | `fork_injects_alternate_event` | `timeline_fork.rs` |
| Timeline | `clone_timeline_replays_entire_run` | `timeline_fork.rs` |
