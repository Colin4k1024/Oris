# Oris Evolution Mechanism Specification


> **Implementation Status: In Progress** 🔄
Source: https://www.notion.so/317e8a70eec5804ca71ae1ae0ea354fa

Last synced: March 3, 2026

## Current Implementation Snapshot (March 3, 2026)

The current `crates/oris-evolution` and `crates/oris-evokernel` layers implement:

- `Gene`, `Capsule`, and `EvolutionEvent` domain models
- append-only JSONL-backed evolution storage
- selector ranking by signal overlap, success rate, reuse count, recency, and environment match
- mutation preparation plus capsule capture and replay-first fallback flows
- caller-attributable replay reuse events when `replay_or_fallback_for_run(...)` is used
- a narrow supervised DEVLOOP path for bounded `docs/*.md` tasks (1 to 3 files) with explicit human approval gating
- `spec_id` linkage for spec-driven mutations
- store-derived Prometheus metrics for replay success, replay reasoning avoided by task class, promotion ratio, revoke frequency, and mutation velocity
- runtime `/metrics` and `/healthz` endpoints that surface the current evolution observability snapshot

Not yet fully implemented in the checked-in code:

- a dedicated confidence lifecycle owned by the evolution crate itself (current decay checks are governor-driven)
- a dedicated runtime signal extraction stage that derives selector inputs automatically
- the full detect/select/mutate pipeline as separate runtime stages
- explicit multi-run promotion enforcement in the evolution crate itself
- OTLP export wiring and dedicated dashboard assets for the evolution-specific metric families

## Related Documents

- [architecture.md](architecture.md)
- [governor.md](governor.md)
- [network.md](network.md)
- [spec.md](spec.md)

## 1. Purpose

This document defines the evolution mechanism of Oris. The system improves
behavior through verified execution outcomes rather than model retraining or
prompt memory.

## 2. Evolution Philosophy

Three rules:

1. Execution produces knowledge.
2. Validation grants inheritance.
3. Selection determines survival.

Only verified execution outcomes may influence future behavior.

## 3. Evolution Loop

```text
Detect
-> Select
-> Mutate
-> Execute
-> Validate
-> Evaluate
-> Solidify
-> Reuse
```

Stage definitions:

- Detect: extract runtime signals from task context.
- Select: match signals against existing genes and rank candidates.
- Mutate: declare intended modification as an evolution transaction.
- Execute: apply the mutation inside sandbox runtime.
- Validate: run build, test, replay, and runtime gates.
- Evaluate: compute success, latency, stability, blast radius, and reproducibility.
- Solidify: emit gene, capsule, and evolution event.
- Reuse: allow future executions to reuse capsules before reasoning.

## 4. Evolution Assets

### 4.1 Gene

Reusable strategy definition representing problem-solving knowledge.

Fields:

- signals
- strategy summary
- constraints
- validation rules

### 4.2 Capsule

Verified execution instance representing proven success.

Contains:

- gene reference
- diff hash
- confidence score
- environment fingerprint
- outcome metrics

### 4.3 Evolution Event

Immutable append-only historical record for lineage tracking, auditability, and
replay reconstruction.

## 5. Signal System

Signals drive evolution reuse. Sources:

- compiler diagnostics
- stack traces
- execution logs
- failure signatures
- performance telemetry

Signal extraction must be deterministic.

## 6. Promotion Rules

Lifecycle:

```text
Candidate -> Promoted -> Revoked -> Archived
```

Target promotion policy requires:

- repeated success
- multi-run validation
- acceptable blast radius
- governor approval

## 7. Confidence Model (Implemented Baseline, Governor-Driven)

Initial confidence derives from validation outcome and increases through reuse
success. It decays with inactivity:

```text
confidence = confidence * e^(-lambda * t)
```

The current repository enforces the baseline decay/regression check through the
governor path. Replay selection now also treats decayed confidence as a first-
class ranking input and can lazily demote stale promoted assets back to
`Quarantined` so they require revalidation before reuse. The evolution crate
still does not own a full background confidence-history scheduler by itself.

## 8. Replay Mechanism

Replay precedes reasoning:

```text
Signal Detection
-> Capsule Lookup
-> Patch Application
-> Validation
```

If replay succeeds, LLM reasoning is skipped.

Agent-facing callers can now convert a replay result into structured
`ReplayFeedback` via `replay_feedback_for_agent(...)`. That boundary exposes a
deterministic task-class id, a human-readable task label, a planner directive
(`SkipPlanner` vs `PlanFallback`), and explicit fallback contract fields:

- `reason_code` (machine-readable, stable fallback reason)
- `fallback_reason` (human-readable fallback explanation)
- `repair_hint` (deterministic minimum repair guidance)
- `next_action` (executable next step)
- `confidence` (`0..100`, contract confidence)

For unknown fallback inputs, the contract is fail-closed by default
(`reason_code=unmapped_fallback_reason`, `next_action=escalate_fail_closed`).
This avoids silently accepting unmapped semantics across API, events, and
callers.

When the caller needs the reuse event tied to a specific execution, use
`replay_or_fallback_for_run(...)` so the resulting `CapsuleReused` event carries
that replay run id in `replay_run_id` while preserving the capsule's original
`run_id`.

## 8.1 Self-Evolution Release Gate Contract (Baseline)

The baseline release gate contract is now machine-readable and defined in
`oris-evokernel` via:

- `ReplayRoiReleaseGateInputContract`
- `ReplayRoiReleaseGateOutputContract`
- `ReplayRoiReleaseGateThresholds`
- `ReplayRoiReleaseGateFailClosedPolicy`

Gate input fixes the core metrics for one replay ROI window:

- `replay_hit_rate` (`replay_success_total / replay_attempts_total`)
- `false_replay_rate` (`replay_failure_total / replay_attempts_total`)
- `reasoning_avoided_tokens`
- `replay_roi`
- `replay_safety` (derived from fail-closed default + rollback readiness + audit
  trail completeness + replay activity presence)

Aggregation dimensions are fixed as:

- `task_class`
- `source_sender_id`

Default threshold policy is conservative and configurable:

- `min_replay_attempts = 3`
- `min_replay_hit_rate = 0.60`
- `max_false_replay_rate = 0.25`
- `min_reasoning_avoided_tokens = 192`
- `min_replay_roi = 0.05`
- `require_replay_safety = true`

Fail-closed defaults are explicit in the contract:

- threshold violation -> `block_release`
- missing metrics -> `block_release`
- invalid metrics -> `block_release`

Release gate evaluator output is deterministic and machine-readable:

- `status = pass` when all threshold checks pass
- `status = fail_closed` when threshold checks fail
- `status = indeterminate` when metrics are missing or invalid (still fail-closed
  for publish decisions)

`failed_checks` is dimension-addressable and stable (for example
`replay_hit_rate_below_threshold`, `missing_replay_attempts`), and
`evidence_refs` always points to metric or threshold dimensions used by each
check.

## 8.2 Supervised DEVLOOP (Bounded Scope + Fail-Closed Taxonomy)

`run_supervised_devloop(...)` remains bounded to docs Markdown tasks under
`docs/` and now enforces deterministic fail-closed constraints before
execution:

- `DocsSingleFile` for one Markdown file
- `DocsMultiFile` for 2 to 3 Markdown files
- reject anything outside that bounded docs scope

- reject out-of-scope proposals (`policy_denied`)
- reject over-budget payload size / validation budget (`policy_denied`)
- reject oversized or boundary-violating patch shapes (`unsafe_patch`)
- fail closed on runtime timeout (`timeout`)
- fail closed on validation failure (`validation_failed`)

The response now carries machine-readable failure metadata through
`failure_contract`:

- `reason_code`
- `failure_reason`
- `recovery_hint`
- `recovery_action`
- `fail_closed`

For audit consistency, failure events are also recorded in
`EvolutionEvent::MutationRejected` with the same `reason_code` and
`recovery_hint`.

## 9. Evolution Store Requirements

Storage must be:

- append-only
- content-addressed
- replayable
- auditable

Recommended layout:

```text
/evolution
  |- genes.json
  |- capsules.json
  `- events.jsonl
```

## 10. Distributed Evolution

Nodes may exchange evolution assets. Remote assets must enter candidate
quarantine before promotion, and local validation is mandatory.

## 11. Evolution Failure Modes

Common risks:

- hallucinated evolution
- overfitting strategies
- mutation cascades
- environment mismatch

These are mitigated by governor controls.

## 12. Success Criteria

Evolution is functioning correctly when:

- repeated failures resolve automatically
- reasoning frequency decreases
- execution latency stabilizes
- behavior converges over time

## 13. Non-Goals

Evolution does not:

- retrain models
- rewrite the kernel autonomously
- trust unvalidated external assets
- modify historical records
