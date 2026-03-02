# Oris - Self-Evolving Agent Kernel Architecture

> **Implementation Status: Implemented** ✅

Source: https://www.notion.so/317e8a70eec5803eb3a4d504c2ba9979

Last synced: March 2, 2026

## 1. Overview

Oris is a deterministic execution kernel designed to evolve software systems
through verified execution history rather than model retraining.

It introduces an evolution layer that converts successful AI coding executions
into reusable, auditable, and transferable intelligence assets.

The system enables:

- Deterministic execution
- Verified evolution
- Experience inheritance
- Replay-driven optimization
- Distributed intelligence sharing

## 2. Architectural Principles

### 2.1 Determinism First

Evolution requires reproducibility. All executions must support:

- replayability
- traceability
- interrupt safety
- state recovery

Non-deterministic execution invalidates evolution.

### 2.2 Execution > Reasoning

Oris improves behavior through:

```text
verified execution reuse
```

not prompt memory or reasoning accumulation.

### 2.3 Append-Only Intelligence

Evolution history is immutable. Experience is never modified, only appended.

### 2.4 Validation-Gated Evolution

Only validated successful executions may enter the evolution system. Prompt
output and explanation never directly affect evolution.

## 3. High-Level System Architecture

```text
User Task
  |
  v
Planner
  |
  v
Executor
  |
  v
Mutation Engine
  |
  v
Sandbox Runtime
  |
  v
Validator
  |
  v
Evolution Governor
  |
  v
Solidify Pipeline
  |
  v
Evolution Store
  |
  v
Selector / Replay
```

## 4. Core Components

### 4.1 Execution Kernel

Responsible for deterministic task execution:

- step execution
- retry policy
- interrupt handling
- execution tracing
- replay support

This is the foundation of trustworthy evolution.

### 4.2 Codex Execution Adapter

Observes AI coding runtime behavior and captures:

- task intent
- generated patches
- execution logs
- validation results
- runtime metadata

The adapter is an execution observer, not an agent.

### 4.3 Mutation Engine

Defines intended system change before execution.

```rust
struct Mutation {
    intent: String,
    target: Target,
    expected_effect: String,
    risk: RiskLevel,
}
```

Purpose:

- auditability
- safety gating
- evolution traceability

### 4.4 Sandbox Runtime

Isolated execution environment. Requirements:

- filesystem isolation
- command whitelist
- timeout enforcement
- restricted permissions

### 4.5 Validator Engine

Typical validators:

- build verification
- test execution
- integration checks
- performance benchmarks
- replay verification

Evolution proceeds only on validation success.

## 5. Evolution System

### 5.1 Evolution Assets

Gene:

- reusable strategy template
- signal match rules
- strategy summary
- validation commands
- constraints

Capsule:

- verified execution instance
- gene reference
- diff hash
- confidence score
- environment fingerprint
- outcome metrics

Evolution Event:

- append-only execution history
- audit trail
- replay lineage
- causal tracking

### 5.2 Solidify Pipeline

Triggered only after validation success.

```text
Validation PASS
-> Signal Extraction
-> Mutation Build
-> Gene Creation
-> Capsule Creation
-> Event Append
```

### 5.3 Evolution Store

Append-only storage:

```text
/evolution
  |- genes.json
  |- capsules.json
  `- events.jsonl
```

Properties:

- immutable history
- deterministic reconstruction
- audit compatibility

## 6. Selection and Replay

### 6.1 Signal Detection

Execution signals are extracted from:

- compiler errors
- runtime failures
- logs
- stack traces

### 6.2 Selection Engine

Genes are ranked by:

- success rate
- reuse frequency
- environment diversity
- recency decay

### 6.3 Replay Executor

Before invoking LLM reasoning:

```text
Detect Signals
-> Select Capsule
-> Apply Known Patch
-> Skip Reasoning
```

Outcome:

- reduced token usage
- faster execution
- behavioral stabilization

## 7. Evolution Governor

The governor prevents evolutionary collapse through:

- mutation rate control
- blast radius control
- diversity preservation
- regression detection
- evolution cooling

## 8. Gene Lifecycle

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

## 9. Oris Evolution Network

Distributed intelligence sharing:

```text
Oris Node <-> Oris Node
```

Each node:

- evolves locally
- publishes promoted assets
- inherits remote experience

Evolution envelope:

```rust
struct EvolutionEnvelope {
    protocol: String,
    sender: NodeId,
    timestamp: Timestamp,
    assets: Vec<Asset>,
}
```

Network operations:

- publish
- fetch
- quarantine

## 10. Trust and Safety

Required safeguards:

- content-addressed assets (SHA256)
- reputation scoring
- candidate quarantine
- deterministic validation replay

## 11. Expected System Behavior

- Early stage: agent relies on reasoning
- Mid stage: repeated problems auto-resolved
- Mature stage: execution reuse dominates reasoning
- Network stage: shared intelligence emerges across nodes

## 12. Repository Module Layout (Recommended)

```text
oris/
|- kernel/
|- executor/
|- sandbox/
|- evolution/
|  |- gene/
|  |- capsule/
|  |- selector/
|  |- governor/
|  `- store/
|- replay/
|- network/
`- adapters/
   `- codex/
```

## 13. Long-Term Vision

```text
Git for Intelligence Evolution
```

A system where verified execution experience becomes transferable machine
capability.
