#!/usr/bin/env python3
"""Generate docs/oris-2.0-kernel-issues.csv for the import script."""
import csv
import os

ISSUES = [
    {
        "title": "[K1] Implement ExecutionStep Contract Freeze",
        "body": """**Description:** Eliminate async side effects, hidden runtime mutations, and adapter nondeterminism by defining a strict, canonical execution contract.

**Tasks:**
- Implement the `ExecutionStep` trait (`execute(state, input) -> StepResult`).
- Enforce pure boundary conditions.
- Define and validate explicit inputs and outputs.

**Acceptance Criteria:** Adapters can only interact through the frozen step contract.""",
        "labels": "epic/K1-Execution-Determinism,type/feature",
        "milestone": "Oris 2.0 Kernel",
    },
    {
        "title": "[K1] Implement Runtime Effect Capture",
        "body": """**Description:** Intercept and record all side effects generated during execution.

**Tasks:**
- Introduce `RuntimeEffect` enum (`LLMCall`, `ToolCall`, `StateWrite`, `InterruptRaise`).
- Implement kernel-level hooks to log all instances of `RuntimeEffect` to the active thread context.

**Acceptance Criteria:** Zero uncaptured side effects leak into the execution state.""",
        "labels": "epic/K1-Execution-Determinism,type/feature",
        "milestone": "Oris 2.0 Kernel",
    },
    {
        "title": "[K1] Build Determinism Guard & Execution Modes",
        "body": """**Description:** Prevent nondeterministic operations from corrupting the state and introduce strict runtime modes.

**Tasks:**
- Implement `KernelMode` (`Normal`, `Record`, `Replay`, `Verify`).
- Add trap handlers to immediately fail execution on clock access, hardware randomness detection, or uncontrolled thread spawning.

**Acceptance Criteria:** Same run guarantees an identical event stream hash; replay mismatches are instantly detected and halted.""",
        "labels": "epic/K1-Execution-Determinism,type/feature",
        "milestone": "Oris 2.0 Kernel",
    },
    {
        "title": "[K2] Build Canonical Execution Log Store",
        "body": """**Description:** Move away from checkpoint-driven state management to an event-sourced log as the source of truth.

**Tasks:**
- Implement `ExecutionLog` struct (`thread_id`, `step_id`, `event_index`, `event`, `state_hash`).
- Refactor current checkpointing to act strictly as an optimization/snapshot layer.""",
        "labels": "epic/K2-Replay-Engine,type/feature",
        "milestone": "Oris 2.0 Kernel",
    },
    {
        "title": "[K2] Develop Replay Cursor Engine",
        "body": """**Description:** Build the core algorithm to reconstruct state from historical events without triggering live side effects.

**Tasks:**
- Implement replay loop: Load checkpoint → Replay events → Inject recorded outputs → Reconstruct state.
- Ensure live tool execution is hard-disabled during replay.""",
        "labels": "epic/K2-Replay-Engine,type/feature",
        "milestone": "Oris 2.0 Kernel",
    },
    {
        "title": "[K2] Implement Replay Verification API",
        "body": """**Description:** Create a diagnostic API to cryptographically verify the integrity of a run.

**Tasks:**
- Expose `oris kernel verify <thread_id>` CLI/API command.
- Implement validation logic for state hash equality, tool checksum matching, and interrupt consistency.""",
        "labels": "epic/K2-Replay-Engine,type/feature",
        "milestone": "Oris 2.0 Kernel",
    },
    {
        "title": "[K2] Enable Branch Replay (Timeline Forking)",
        "body": """**Description:** Allow execution timelines to fork from a specific historical checkpoint.

**Tasks:**
- Implement logic to resume replay from checkpoint N, inject an alternate decision, and fork the event stream.

**Acceptance Criteria:** Reasoning timelines can be successfully reconstructed, audited, simulated, and forked.""",
        "labels": "epic/K2-Replay-Engine,type/feature",
        "milestone": "Oris 2.0 Kernel",
    },
    {
        "title": "[K3] Define and Store Kernel Interrupt Object",
        "body": """**Description:** Standardize how interrupts are represented and persisted.

**Tasks:**
- Implement `Interrupt` struct (`id`, `thread_id`, `kind`, `payload_schema`, `created_at`).
- Ensure interrupts are flushed and stored alongside execution checkpoints.""",
        "labels": "epic/K3-Interrupt-Kernel,type/feature",
        "milestone": "Oris 2.0 Kernel",
    },
    {
        "title": "[K3] Implement Execution Suspension State Machine",
        "body": """**Description:** Handle the safe teardown of workers when an execution is paused.

**Tasks:**
- Implement kernel state transitions: `Running` → `Suspended` → `WaitingInput`.
- Ensure the worker safely exits and releases resources upon suspension.""",
        "labels": "epic/K3-Interrupt-Kernel,type/feature",
        "milestone": "Oris 2.0 Kernel",
    },
    {
        "title": "[K3] Enforce Replay-Based Resume Semantics",
        "body": """**Description:** Guarantee that resuming a suspended state does not rely on active memory.

**Tasks:**
- Implement resume logic strictly as: Replay + Inject Decision.
- Write tests to guarantee resuming N times yields identical results (idempotent resumes).""",
        "labels": "epic/K3-Interrupt-Kernel,type/feature",
        "milestone": "Oris 2.0 Kernel",
    },
    {
        "title": "[K3] Build Unified Interrupt Routing Layer",
        "body": """**Description:** Create a single resolver for all external interrupt sources.

**Tasks:**
- Implement `InterruptResolver` trait (`async fn resolve(interrupt) -> Value`).
- Map inputs from UI, agents, policy engines, and APIs through the resolver.

**Acceptance Criteria:** A process can be suspended, memory completely cleared, and successfully resumed days later.""",
        "labels": "epic/K3-Interrupt-Kernel,type/feature",
        "milestone": "Oris 2.0 Kernel",
    },
    {
        "title": "[K4] Define and Enforce Plugin Categories",
        "body": """**Description:** Standardize the types of plugins the kernel will recognize and load.

**Tasks:**
- Implement interfaces for `NodePlugin`, `ToolPlugin`, `MemoryPlugin`, `LLMAdapter`, and `SchedulerPlugin`.""",
        "labels": "epic/K4-Plugin-Runtime,type/feature",
        "milestone": "Oris 2.0 Kernel",
    },
    {
        "title": "[K4] Implement Plugin Determinism Declarations",
        "body": """**Description:** Force plugins to declare their behavioral boundaries to the kernel.

**Tasks:**
- Require `PluginMetadata` (`deterministic`, `side_effects`, `replay_safe`) for all plugins.
- Build kernel enforcement logic (replay substitution, sandbox routing).""",
        "labels": "epic/K4-Plugin-Runtime,type/feature",
        "milestone": "Oris 2.0 Kernel",
    },
    {
        "title": "[K4] Build Plugin Execution Sandbox",
        "body": """**Description:** Isolate plugin execution based on safety requirements.

**Tasks:**
- Implement execution mode routers (`InProcess`, `IsolatedProcess`, `Remote`).
- (Spike/Future): Blueprint WASM execution mode.""",
        "labels": "epic/K4-Plugin-Runtime,type/feature",
        "milestone": "Oris 2.0 Kernel",
    },
    {
        "title": "[K4] Implement Plugin Version Negotiation & Dynamic Registry",
        "body": """**Description:** Prevent runtime corruption through strict validation and enable hot-loading.

**Tasks:**
- Add validation for `plugin_api_version`, `kernel_compat`, and `schema_hash` on load.
- Upgrade `NodePluginRegistry` to support dynamic hot-loading and unloading of validated plugins.

**Acceptance Criteria:** Third-party tools can be loaded and executed without threatening kernel determinism.""",
        "labels": "epic/K4-Plugin-Runtime,type/feature",
        "milestone": "Oris 2.0 Kernel",
    },
    {
        "title": "[K5] Finalize Lease-Based Execution",
        "body": """**Description:** Formalize ownership of execution attempts.

**Tasks:**
- Implement `WorkerLease` to strictly enforce single-owner execution.
- Build lease expiry, recovery, and replay-restart logic.""",
        "labels": "epic/K5-Distributed-Execution,type/feature",
        "milestone": "Oris 2.0 Kernel",
    },
    {
        "title": "[K5] Implement Zero-Data-Loss Failure Recovery Loop",
        "body": """**Description:** Automate the recovery pipeline when a worker abruptly crashes.

**Tasks:**
- Implement the crash recovery pipeline: Lease expires → Checkpoint reloads → Execution replays → Dispatched to new worker.""",
        "labels": "epic/K5-Distributed-Execution,type/feature",
        "milestone": "Oris 2.0 Kernel",
    },
    {
        "title": "[K5] Build Context-Aware Scheduler Kernel",
        "body": """**Description:** Upgrade the dispatcher to route tasks intelligently across the cluster.

**Tasks:**
- Inject awareness of tenant limits, priority queues, plugin requirements, interrupt backlogs, and specific worker capabilities into the scheduling algorithm.""",
        "labels": "epic/K5-Distributed-Execution,type/feature",
        "milestone": "Oris 2.0 Kernel",
    },
    {
        "title": "[K5] Implement Safe Backpressure Engine & Kernel Observability",
        "body": """**Description:** Protect the cluster from overload and provide deep kernel insights (not just standard logs).

**Tasks:**
- Build safe rejection mechanisms returning explicit `tenant_limit` reasons.
- Expose telemetry for Reasoning Timeline, Lease Graph, Replay Cost, and Interrupt Latency.

**Acceptance Criteria:** `kill -9` on an active worker → cluster restarts → resumes exactly where it left off with an identical reasoning outcome.""",
        "labels": "epic/K5-Distributed-Execution,type/feature",
        "milestone": "Oris 2.0 Kernel",
    },
]

def main():
    out_path = os.path.join(os.path.dirname(__file__), "..", "docs", "oris-2.0-kernel-issues.csv")
    with open(out_path, "w", encoding="utf-8", newline="") as f:
        writer = csv.DictWriter(f, fieldnames=["title", "body", "labels", "milestone"], quoting=csv.QUOTE_ALL)
        writer.writeheader()
        writer.writerows(ISSUES)
    print(f"Wrote {len(ISSUES)} issues to {out_path}")

if __name__ == "__main__":
    main()
