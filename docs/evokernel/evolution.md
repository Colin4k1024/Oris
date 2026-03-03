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
- `spec_id` linkage for spec-driven mutations

Not yet fully implemented in the checked-in code:

- confidence decay over time
- a dedicated runtime signal extraction stage that derives selector inputs automatically
- the full detect/select/mutate pipeline as separate runtime stages
- explicit multi-run promotion enforcement in the evolution crate itself

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

## 7. Confidence Model (Planned)

Initial confidence derives from validation outcome and increases through reuse
success. It decays with inactivity:

```text
confidence = confidence * e^(-lambda * t)
```

This prevents stale evolution dominance.

## 8. Replay Mechanism

Replay precedes reasoning:

```text
Signal Detection
-> Capsule Lookup
-> Patch Application
-> Validation
```

If replay succeeds, LLM reasoning is skipped.

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
