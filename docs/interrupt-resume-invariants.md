# Interrupt / Resume Invariants

## Overview

Oris uses a **two-layer interrupt design**:

1. **Kernel-level** (`oris-kernel`) — The `Kernel` driver loops on `StepFn::next()`. When a step returns `Next::Interrupt(InterruptInfo)`, the driver persists an `Event::Interrupted` to the event log, optionally saves a snapshot, records to the effect sink, and returns `RunStatus::Blocked`. Resume is performed via `Kernel::resume()`, which appends `Event::Resumed` then re-enters the run loop.

2. **Graph-level** (`oris-runtime/graph/interrupts`) — The `interrupt()` async function uses thread-local `InterruptContext` with positional resume values. When no resume value is available at the current index, it returns `Err(InterruptError)`, which the graph execution layer catches to produce an interrupt.

The `KernelInterrupt` (K3) and `ExecutionSuspension` provide richer objects for the execution-runtime control plane, supporting checkpoint capture and worker lifecycle management at a higher layer.

## Kernel-Level Interrupt Flow

### Interrupting

```
StepFn::next(&state)
  -> Next::Interrupt(InterruptInfo { value })
    -> driver records RuntimeEffect::InterruptRaise
    -> driver appends Event::Interrupted { value } to EventStore
    -> driver applies event via Reducer
    -> driver optionally saves snapshot
    -> driver returns RunStatus::Blocked(BlockedInfo { interrupt: Some(info) })
```

### Resuming

```
Kernel::resume(run_id, initial_state, Signal::Resume(value))
  -> appends Event::Resumed { value } to EventStore   // BEFORE re-entering loop
  -> calls run_loop()
    -> restore_state(): loads snapshot + replays tail events (including Resumed)
    -> loops on StepFn::next(&state) until Blocked/Complete/Failed
```

## Core Invariants

### INV-I1: Interrupted Event Always Persisted Before Returning Blocked

When `StepFn::next()` returns `Next::Interrupt`, the driver **must** append `Event::Interrupted` to the event store before returning `RunStatus::Blocked`. This guarantees crash recovery can detect the interrupt.

**Source:** `crates/oris-kernel/src/kernel/driver.rs`

### INV-I2: Resumed Event Appended Before Re-entering the Run Loop

`Kernel::resume()` appends `Event::Resumed { value }` to the event store **before** calling `run_loop()`. This ensures the resume decision is durably recorded regardless of what happens during subsequent execution.

**Source:** `crates/oris-kernel/src/kernel/driver.rs`

### INV-I3: Interrupt Consistency (LIFO Matching)

Every `Interrupted` event must be matched by a corresponding `Resumed` event. Matching follows LIFO (stack-based) order. `ReplayVerifier::verify_interrupt_consistency()` enforces this by pushing `Interrupted` events onto a stack and popping on `Resumed`.

Violations produce `VerificationFailure::UnmatchedInterrupt` or `UnmatchedResume`.

**Source:** `crates/oris-kernel/src/kernel/replay_verifier.rs`

### INV-I4: Resume Only Valid After Blocked Status

Resume is only valid when the kernel is in `Blocked` status. Calling resume on a completed or running kernel is a protocol violation.

**Source:** `docs/kernel-api.md` Section 11

### INV-I5: Resume Idempotency

Resuming from the same event log with the same `ResumeDecision` yields identical state and event count. `ReplayResume::verify_idempotent()` confirms this by executing two independent resumes and comparing results.

**Source:** `crates/oris-kernel/src/kernel/replay_resume.rs`

### INV-I6: Snapshot Saved on Interrupt

When an interrupt occurs, a snapshot is saved before returning `Blocked`. This enables efficient resume without replaying the entire event log.

**Source:** `crates/oris-kernel/src/kernel/driver.rs` — verified by `interrupt_saves_snapshot_before_returning_blocked` test

### INV-I7: Action Pairing Preserved Across Interrupts

Every `ActionRequested` event is paired with exactly one `ActionSucceeded` or `ActionFailed`, even when an interrupt occurs between them. The driver completes the action before processing the interrupt.

**Source:** `crates/oris-kernel/src/kernel/driver.rs`

## State Machines

### KernelInterruptStatus

```
Pending ──→ Resolving ──→ Resolved
                      ├──→ Rejected
                      └──→ Expired
```

| Transition | Precondition | Error on violation |
|-----------|-------------|-------------------|
| `start_resolving()` | Must be `Pending` | `InvalidStatusTransition` |
| `resolve()` | Must be `Resolving` | `InvalidStatusTransition` |
| `can_resume()` | Requires `checkpoint.is_some() && is_pending()` | — |

Additional properties:
- `requires_input()` returns `true` for `HumanInTheLoop`, `ApprovalRequired`, `ToolCallWaiting`, `ErrorRecovery`
- `is_checkpoint()` returns `true` only for `Checkpoint`

**Source:** `crates/oris-kernel/src/kernel/kernel_interrupt.rs`

### ExecutionSuspensionState

```
Running ──→ Suspended ──→ WaitingInput ──→ Running (resumed)
```

| Transition | Precondition | Error on violation |
|-----------|-------------|-------------------|
| `suspend()` | Must be `Running` | Invalid transition error |
| `wait_input()` | Must be `Suspended` | Invalid transition error |
| `resume()` | Must be `WaitingInput` | Invalid transition error |

Strict linear state machine — no skipping states. `is_suspended()` returns `true` for both `Suspended` and `WaitingInput`.

**Source:** `crates/oris-kernel/src/kernel/execution_suspension.rs`

## Test Coverage

| Invariant | Test | File |
|-----------|------|------|
| INV-I1, INV-I2 | `run_until_blocked_then_resume` | `driver.rs` |
| INV-I3 | `verify_interrupt_consistency_unmatched_interrupt` | `replay_verifier.rs` |
| INV-I3 | `verify_interrupt_consistency_ok` | `replay_verifier.rs` |
| INV-I5 | `resume_injects_decision` | `replay_resume.rs` |
| INV-I5 | `resume_idempotent_twice` | `replay_resume.rs` |
| INV-I5 | `verify_idempotent_returns_true` | `replay_resume.rs` |
| INV-I6 | `interrupt_saves_snapshot_before_returning_blocked` | `driver.rs` |
| INV-I7 | `retry_then_success_has_single_terminal_success_event` | `driver.rs` |
| INV-I7 | `retry_exhausted_has_single_terminal_failed_event` | `driver.rs` |
| State machine | `kernel_interrupt_status_transition` | `kernel_interrupt.rs` |
| State machine | `create_kernel_interrupt` | `kernel_interrupt.rs` |
| State machine | `kernel_interrupt_with_checkpoint` | `kernel_interrupt.rs` |
| State machine | `interrupt_requires_input` | `kernel_interrupt.rs` |
| State machine | `new_suspension_is_running` | `execution_suspension.rs` |
| State machine | `running_to_suspended_transition` | `execution_suspension.rs` |
| State machine | `suspended_to_waiting_input` | `execution_suspension.rs` |
| State machine | `waiting_input_to_running_resume` | `execution_suspension.rs` |
| State machine | `invalid_transition_running_to_waiting` | `execution_suspension.rs` |
| Interrupt store | `save_and_load_interrupt` | `interrupt.rs` |
| Interrupt store | `load_for_run_filters` | `interrupt.rs` |
| Interrupt store | `delete_removes_interrupt` | `interrupt.rs` |
| Resolver | `resolver_routes_by_source` | `interrupt_resolver.rs` |

## Code References

| File | Purpose |
|------|---------|
| `crates/oris-kernel/src/kernel/driver.rs` | `Kernel::run_until_blocked()`, `resume()` |
| `crates/oris-kernel/src/kernel/interrupt.rs` | `Interrupt` struct, `InterruptStore` trait |
| `crates/oris-kernel/src/kernel/interrupt_resolver.rs` | `InterruptResolver` trait, unified routing |
| `crates/oris-kernel/src/kernel/kernel_interrupt.rs` | `KernelInterrupt`, status state machine (K3-a) |
| `crates/oris-kernel/src/kernel/execution_suspension.rs` | `ExecutionSuspension` worker lifecycle |
| `crates/oris-kernel/src/kernel/replay_resume.rs` | `ReplayResume` idempotent resume |
| `crates/oris-kernel/src/kernel/step.rs` | `Next::Interrupt(InterruptInfo)` |
| `crates/oris-kernel/src/kernel/runner.rs` | `KernelRunner` sync/async wrappers |
| `crates/oris-kernel/src/kernel/event.rs` | `Event::Interrupted`, `Event::Resumed` |
| `crates/oris-runtime/src/graph/interrupts/` | Graph-level interrupt mechanism |
