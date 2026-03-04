# Oris EvoKernel - Self-Evolving Agent System Design Document

Local mirror of the Notion source:
https://www.notion.so/317e8a70eec5809c85e1f52aa03870e4

Last synced: March 2, 2026

Version: Draft v0.1

Purpose: Consolidate architectural discussions on implementing EvoMap-class
self-evolution capability using AI coding plus the Oris kernel.

## Repository Mapping (March 2, 2026)

The current repository already contains the first executable slice of this
design:

- `crates/oris-kernel` is the deterministic execution substrate.
- `crates/oris-execution-runtime` is the graph-agnostic runtime control plane.
- `crates/oris-evolution` defines genes, capsules, events, projections, and the
  append-only JSONL evolution store.
- `crates/oris-sandbox` applies mutations in a constrained local sandbox.
- `crates/oris-evokernel` wires mutation capture, validation, capsule
  construction, and replay-first reuse.
- `crates/oris-runtime/src/evolution.rs` re-exports the EvoKernel API as
  `oris_runtime::evolution` behind the experimental `evolution-experimental`
  feature.
- The checked-in example and smoke test use `full-evolution-experimental` when
  they need the aggregate `oris-runtime` experimental facade set (`evolution`,
  `governor`, `evolution_network`, `economics`, `spec_contract`, and
  `agent_contract`) at the same time.

Today the implementation covers the execution hook, append-only storage,
selector, and replay-first behavior in an experimental form. Governance,
network propagation, and stronger isolation remain design targets rather than
fully implemented capabilities.

## 1. Executive Summary

This document defines how to evolve Oris from an execution framework into a
self-evolving agent operating system capable of:

- Capturing successful AI coding executions
- Converting execution outcomes into reusable evolution assets
- Applying selection pressure over accumulated experience
- Enabling deterministic replay and inheritance
- Supporting multi-agent evolutionary knowledge sharing

The goal is post-training intelligence evolution, where improvement happens
through execution history rather than model retraining.

## 2. Core Concept

Traditional AI coding:

```text
Prompt -> LLM -> Code -> Done
```

Self-evolving system:

```text
Task
-> Execution
-> Validation
-> Evolution Asset
-> Selection
-> Behavior Change
```

Key principle:

> Intelligence improves through verified execution reuse, not reasoning repetition.

## 3. Evolution Architecture Overview

### System Loop

```text
User Task
   v
Planner
   v
Executor
   v
Mutation Engine
   v
Sandbox Runtime
   v
Validation
   v
Evolution Governor
   v
Solidify
   v
Evolution Store
   v
Selector
   v
Future Execution
```

This forms a continuous evolution feedback loop.

## 4. Evolution Asset Model

Self-evolution relies on structured, machine-verifiable assets.

### 4.1 Gene - Strategy DNA

Reusable problem-solving strategy.

Example:

```text
Signal:
  rust borrow error

Strategy:
  reduce lifetime scope
```

Structure:

- signals
- strategy
- validation rules
- constraints

Represents how problems are solved.

### 4.2 Capsule - Verified Experience

A successful real execution instance.

Contains:

- applied gene
- code diff hash
- confidence score
- environment fingerprint
- outcome metrics

Represents proof that strategy worked.

### 4.3 Evolution Event

Append-only historical record:

- intent
- signals detected
- genes used
- execution result
- validation outcome

Provides auditability and replay.

## 5. Codex Evolution Hook

### Critical Rule

Evolution must trigger only after validation success.

Correct hook position:

```text
Generate Patch
-> Apply Patch
-> Execute
-> Validation PASS
-> Evolution Hook
```

Never evolve from prompts or explanations.

### Adapter Responsibilities

Capture:

- task intent
- generated diff
- execution logs
- validation result
- runtime metadata

Transform execution into evolution assets automatically.

## 6. Solidify Pipeline

```text
Validation PASS
-> Signal Extraction
-> Mutation Creation
-> Gene Generation
-> Capsule Creation
-> Event Append
```

Solidification converts temporary success into permanent intelligence.

## 7. Evolution Store

The store must be append-only.

Recommended structure:

```text
/evolution
 |- genes.json
 |- capsules.json
 `- events.jsonl
```

Current repository layout:

```text
.oris/evolution/
  events.jsonl
  genes.json
  capsules.json
  LOCK
```

Properties:

- immutable history
- deterministic replay
- audit capability
- trust preservation

`events.jsonl` is the source of truth. `genes.json` and `capsules.json` are
projection caches rebuilt from the log.

## 8. Selection Engine

Evolution occurs through selection pressure.

Gene scoring factors:

- success rate
- reuse frequency
- environment diversity
- recency decay

Execution preference shifts from reasoning toward reuse:

```text
selection > reasoning
```

## 9. Replay Executor

Before invoking LLM reasoning:

```text
Detect Signals
-> Find Capsule
-> Apply Known Patch
-> Skip Reasoning
```

Expected result:

- reduced token usage
- faster execution
- stabilized behavior

Current behavior: `StoreReplayExecutor` attempts capsule replay first and falls
back to the planner when patch application or validation fails. Callers that
need explicit replay auditability can supply a replay run id through
`replay_or_fallback_for_run(...)`; the convenience `replay_or_fallback(...)`
path auto-generates one.

## 10. Evolution Governor (Stability Layer)

Self-evolving systems collapse without governance.

Governor responsibilities:

### 10.1 Mutation Rate Control

Limit evolution speed and prevent strategy drift.

### 10.2 Blast Radius Control

Evaluate impact scope:

- files changed
- lines modified

Large mutations require stricter promotion.

### 10.3 Diversity Preservation

Avoid monoculture failure.

Maintain an exploration rate of roughly 10-20 percent.

### 10.4 Regression Detection

Automatically revoke degrading genes.

### 10.5 Evolution Cooling

Introduce cooldown after promotion to prevent mutation cascades.

## 11. Gene Lifecycle

```text
Candidate
   v
Promoted
   v
Revoked
   v
Archived
```

Promotion requires repeated verified success.

## 12. Network Evolution (Oris Evolution Network)

Transition from single-agent learning to shared intelligence.

```text
Oris Node <-> Oris Node <-> Oris Node
```

Each node:

- evolves locally
- publishes assets
- inherits remote experience

### Evolution Envelope

Standard transmission unit containing:

- sender
- timestamp
- assets
- protocol metadata

### Network Operations

#### Publish

Share promoted capsules.

#### Fetch

Retrieve remote experience based on signals.

#### Local Validation

Remote assets enter quarantine before promotion.

## 13. Trust and Safety Layer

Required protections:

- content-addressed assets (hash verification)
- node reputation scoring
- candidate quarantine
- deterministic validation replay

These controls prevent malicious evolution propagation.

## 14. Expected System Evolution

### Phase 1

Agent writes code.

### Phase 2

Successful executions become reusable assets.

### Phase 3

Repeated issues become auto-resolved.

### Phase 4

Reasoning demand decreases.

### Phase 5

Network intelligence emerges.

## 15. End State Vision

Final system:

```text
Oris =
Deterministic Execution
+
Evolution Memory
+
Selection Pressure
+
Network Sharing
```

Result:

> A self-improving software factory.

Intelligence scaling is achieved through evolution rather than model size.

## 16. Immediate Implementation Priorities

1. Codex execution hook
2. Solidify pipeline
3. Evolution store
4. Selector engine
5. Evolution governor
6. Replay executor
7. Network envelope protocol

## 17. Strategic Outcome

Successful implementation transforms Oris into:

```text
Git for Intelligence Evolution
```

A platform where verified experience becomes transferable machine capability.

## Related Local Mirrors

The linked Notion subpages are mirrored under `docs/evokernel/`:

- `docs/evokernel/architecture.md`
- `docs/evokernel/evolution.md`
- `docs/evokernel/governor.md`
- `docs/evokernel/network.md`
- `docs/evokernel/economics.md`
- `docs/evokernel/kernel.md`
- `docs/evokernel/implementation-roadmap.md`
- `docs/evokernel/bootstrap.md`
- `docs/evokernel/agent.md`
- `docs/evokernel/devloop.md`
- `docs/evokernel/spec.md`
- `docs/evokernel/vision.md`
- `docs/evokernel/founding-paper.md`
