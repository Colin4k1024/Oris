# Oris Kernel API (2.0)

This document defines the **minimal complete set** of interfaces that constitute the Oris execution kernel. It aligns with the axioms in [ORIS_2.0_STRATEGY.md](ORIS_2.0_STRATEGY.md) and ensures the runtime satisfies four properties:

1. **Run** — Long-running execution with identity
2. **Event Log** — Source of truth (auditable)
3. **Replay** — Replayable and recoverable
4. **Action** — Single channel for tools and external world (governable)

Graph, Agent, and Planner sit **above** the kernel; they compile down to the kernel’s step and action model.

---

## Axioms (A1–A9) and kernel mapping

- **A1–A2** (reasoning state, interrupt semantics) → Run identity, Event log (Interrupted/Resumed), StepFn returning Next::Interrupt.
- **A3** (replay, effect boundaries) → EventStore as source of truth; Reducer projects events to state; replay uses only stored events (no external action execution).
- **A4** (unit of execution = step) → StepFn and Next (Emit / Do(Action) / Interrupt / Complete).
- **A5** (state = graph state + messages, versioned) → State with `version()`; Snapshot carries `at_seq`.
- **A6** (tool/LLM as system action) → Action enum and ActionExecutor; results only as events (ActionSucceeded/ActionFailed).
- **A9** (governance) → Policy trait (authorize, retry_strategy, budget).

---

## 1. Run identity

- **RunId** — Identifies a long-running run (maps to current `thread_id`).
- **StepId** — Identifies a step (node + attempt).
- **Seq** — Monotonically increasing event sequence number per run. EventStore assigns or requires seq for ordering.

---

## 2. EventStore (source of truth)

**Constraints (must be enforced in implementation and tests):**

- **append**: Either all events in the batch succeed or none do (atomicity).
- Every event has a **seq** (assigned by the store or by the caller in a defined way).
- **scan(run_id, from)** returns events in **ascending seq order**.

Events are the only source of truth; state is derived by reduction.

---

## 3. SnapshotStore (optimization layer)

- Snapshots are an **optimization**, not the source of truth. The **source of truth** is the event log.
- **Rebuild semantics**: To obtain the current state for a run, the kernel does: **state = load_latest(run_id).state + replay(events, from = at_seq + 1)**. If there is no snapshot, state = initial_state and replay starts from seq 1. The snapshot only skips already-applied events; correctness depends on the event stream.
- Every **Snapshot** must include **at_seq: Seq** — the seq up to which state has been projected. Recovery: load snapshot, then apply only events with seq > at_seq.
- **Implementations**: `kernel::InMemorySnapshotStore<S>` stores one snapshot per run. Graph `StateSnapshot` has optional `at_seq`; when the graph uses an event store, checkpoints saved at interrupt carry `at_seq` from the store head (e.g. `event_store.head(run_id)`).

---

## 4. Action results only as events

- **ActionExecutor** must not mutate state directly.
- All external effects (tool call, LLM call, etc.) are recorded as:
  - **ActionRequested** (before execution)
  - **ActionSucceeded { output }** or **ActionFailed { error }** (after execution)
- Replay uses these stored results and does **not** call ActionExecutor.

### 4.1 Non-determinism boundary (非确定性边界)

All non-deterministic inputs must go through **Action** and be recorded as events. Replay must never call the real executor.

- **In scope**: LLM responses, tool I/O, wall-clock time, random numbers, external signals (e.g. human approval). These must be performed as Actions and produce **ActionRequested** → **ActionSucceeded** or **ActionFailed**.
- **Replay**: When rebuilding state from the event log (or from a snapshot + tail events), the kernel only applies stored events; it does **not** invoke ActionExecutor. Thus replay is deterministic and has no side effects.

---

## 5. Policy (governance)

- The kernel must have a **Policy** layer (even if a minimal implementation).
- **authorize(run_id, action, ctx)** — Decide whether the action is allowed.
- **retry_strategy(error, action)** / **retry_strategy_attempt(error, action, attempt)** — Decide retry, retry-after-ms, or fail.
- **budget()** (optional) — Cost/usage limits (e.g. max_tool_calls, max_llm_tokens).

**Default**: `AllowAllPolicy` allows all actions and fails on first error. **Enterprise**: Use `AllowListPolicy` (allow/deny by tool or provider), `RetryWithBackoffPolicy` (wrap another policy for retries with backoff), and optionally a budget; see `kernel::policy` and `kernel::stubs`.

---

## 6. Module layout (in-repo)

- **`oris_runtime::kernel`** — All kernel types and traits (identity, event, snapshot, state, reducer, action, step, policy, driver).
- **graph / agent / tools** — Unchanged stable API; later, Graph/Agent compile to StepFn, tools implement ActionExecutor.

---

## 7. Relation to current APIs

- **Checkpoint / thread_id** — Today’s checkpointer is snapshot-only; kernel adds EventStore and Snapshot with `at_seq`. Existing `thread_id` ↔ RunId.
- **Interrupt / resume** — Map to events Interrupted / Resumed; StepFn returns Next::Interrupt; driver exposes resume(run_id, signal).
- **RunStatus** — Standardized status: `Completed`, `Blocked(BlockedInfo)` (interrupt or WaitSignal), `Running` (optional), `Failed { recoverable: bool }` (optional).
- **Trace (TraceEvent)** — Current trace events (StepCompleted, InterruptReached, ResumeReceived) are a subset of kernel Event types; kernel Event covers also StateUpdated, ActionRequested/Succeeded/Failed, Completed.

---

## 8. Event-first pilot path (Phase 2)

One execution path satisfies “write event → then execute → then write result” (A2/A3):

- **Path**: Graph execution with interrupts: `invoke_with_config_interrupt` when the graph is compiled with `.with_event_store(store)`.
- **RunId**: `thread_id` from checkpoint config.
- **Events written**: Before execution, any resume values produce `Resumed` events. During execution, per node: `ActionRequested` (action_id = run_id-node, payload = state) → node runs → `ActionSucceeded` (output = state update) or `ActionFailed` (on error or on interrupt); then `StateUpdated`; on interrupt, `ActionFailed { error: "Interrupt" }` then `Interrupted { value }` so every ActionRequested is paired; on normal end, `Completed`.
- **EventStore**: Use `kernel::InMemoryEventStore` (or any `EventStore` implementation). When event_store is set, checkpoints saved at interrupt include `at_seq` (from EventStore head).

---

## 9. Replay-only mode (Phase 3)

- **`Kernel::replay(run_id, initial_state)`**: Scans all events for the run (from seq 1), applies each with the Reducer in order. Does **not** call ActionExecutor; any ActionRequested is satisfied by the following ActionSucceeded/ActionFailed already in the log (reducer applies them).
- **`Kernel::replay_from_snapshot(run_id, initial_state)`**: If a snapshot exists for the run, starts from `snap.state` and applies only events with seq > snap.at_seq; otherwise same as replay. Rebuild semantics: state = snap + events(from=at_seq+1).
- **Use case**: Reproducible state from history, audit, and recovery without re-executing external actions (A3).
- **Tests**: `test_replay_reproduces_state` (graph + replay state match); `replay_no_side_effects` (executor 0 calls); `replay_state_equivalence` (same log → same state); `replay_from_snapshot_applies_tail_only`.

---

## 10. Event-first execution model (事件优先执行模型)

Execution is **event-first**: the event log is the single source of truth; state is always derived by reducing events.

1. **Append order**  
   The driver appends events in a fixed order. For each external action: append **ActionRequested** → execute (or retry under policy) → append **ActionSucceeded** or **ActionFailed**. Every ActionRequested is paired with exactly one result event so that replay never sees an orphan request.

2. **Replay never executes actions**  
   Replay only scans stored events and applies them with the Reducer. It does **not** call ActionExecutor. Non-determinism (LLM, tools, time, etc.) is confined to Actions; their outcomes are already in the log.

3. **at_seq and snapshots**  
   A snapshot stores `state` and `at_seq` (the seq up to which that state was built). Rebuild = load snapshot state, then replay events with seq > at_seq. Snapshots are an optimization; correctness depends only on the event stream.

4. **Failure and retry**  
   On executor error the driver appends **ActionFailed** (so the log stays consistent), then consults Policy `retry_strategy_attempt`. If the policy returns **Fail**, the run returns `RunStatus::Failed { recoverable }`; otherwise the driver retries (with optional backoff). The policy is responsible for eventually returning Fail so the loop terminates.

### 10.1 Event field semantics

- **StateUpdated.step_id** — The step/node that produced this update (e.g. the graph node that just ran). For the graph adapter, the next node to run (cursor) is carried in the payload under `next_node` when present (envelope format).
- **ActionRequested / ActionSucceeded / ActionFailed** — One ActionRequested is appended per logical action. On executor retries the driver does not append another Requested; after retries complete, a single ActionFailed is appended (no duplicate Requested).

---

## 11. Standardized run status (标准化运行状态)

`RunStatus` describes the outcome of `run_until_blocked` or `resume`:

| Status | Meaning | Next steps |
|--------|---------|------------|
| **Completed** | The step fn returned `Next::Complete`; the run is done. | None. |
| **Blocked(BlockedInfo)** | The step fn returned `Next::Interrupt` or is waiting on a signal. | Call `resume(run_id, signal)` when the interrupt is resolved or the signal arrives. |
| **Running** | Optional; used when yielding before blocking. | Call `run_until_blocked` again (or continue the loop). |
| **Failed { recoverable }** | An action failed and the policy chose not to retry (or retries were exhausted). | If `recoverable` is true, the run may be retried (e.g. resume with a new signal or restart from checkpoint). If false, the run should not be retried. |

- **Resume**: Only valid after **Blocked**. Append a **Resumed** (or **Signal**) event, then run until the next Blocked or Completed or Failed.
- **Retry**: Handled inside the driver via Policy `retry_strategy` (Retry / RetryAfterMs / Fail). After **Failed**, the application may retry the whole run (e.g. from a snapshot) when `recoverable` is true.
