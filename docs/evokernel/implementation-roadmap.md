# Oris EvoKernel - 90 Day Implementation Roadmap

Source: https://www.notion.so/317e8a70eec580cfb252f8b09a40d21c

Last synced: March 5, 2026

## Current Baseline Update (March 5, 2026)

The repository now passes a concrete self-evolution baseline.

What is already true in the checked-in code:

- verified mutations can be captured into the evolution store
- the same learned task can replay on a later run
- repeated learned tasks increase replay utilization in regression coverage
- remote assets remain quarantined until a local replay validates them
- replay behavior survives restart when the same persistent store is reused
- unrelated tasks do not falsely replay in the current black-box suite

This means the accurate current state is:

> constrained replay-driven self-evolution

It is not yet accurate to describe the repository as a closed-loop autonomous
self-development system.

The acceptance baseline and current test gate now live in:

- `docs/evokernel/self-evolution-acceptance-checklist.md`

The staged evolution tracking issue is now complete:

- GitHub issue `#86`, `[EVO] Track staged self-evolution from replay baseline to supervised devloop` (closed)
- Released in `oris-runtime v0.14.0`

## Near-Term Execution Priorities

Before pushing further into autonomy, the next implementation priority order is:

1. Expand task-class generalization beyond current normalized signal matching.
2. Introduce continuous confidence control, including decay, revalidation, and demotion.
3. Close the agent feedback loop so replay results measurably reduce future reasoning.
4. Move toward supervised DEVLOOP orchestration for bounded development tasks.
5. Harden federated evolution with stronger attribution, economics, and revocation propagation.

Completed stage-tracking issue split (all closed and shipped in `oris-runtime v0.14.0`):

- `#87` `EVO-01`: expand deterministic task-class replay matching beyond normalized signals
- `#88` `EVO-02`: add a continuous confidence lifecycle for evolution assets
- `#89` `EVO-03`: close the replay feedback loop for agent callers
- `#90` `EVO-04`: introduce supervised DEVLOOP orchestration for bounded tasks
- `#91` `EVO-05`: harden federated evolution attribution, economics, and revocation

## EvoMap Compatibility Delta Stream

### Historical phase (March 5 to March 29, 2026)

The original delta stream focused on foundation compatibility:

- `/a2a/*` namespace facade
- `fetch -> task/work claim -> complete`
- `/a2a/heartbeat` keepalive payload
- `gep-a2a` envelope compatibility
- observability/docs parity for compatibility endpoints

That phase corresponds to the earlier EvoMap alignment issues (`#110` to `#119`), which are now closed.

### Active phase (as of March 9, 2026)

Current gap closure is tracked by semantic parity issues:

- `#148` to `#155` (label: `evomap-gap`, milestone: `EvoMap Gap Closure`)

Use this document as source-of-truth for unified code-to-issue mapping:

- `docs/evomap-gap-unified-alignment.md`

Issue import/sync path remains:

- `bash scripts/import_issues_from_csv.sh --repo Colin4k1024/Oris --create-milestones --create-labels`

## 1. Objective

Convert the architecture into a production-ready self-evolving kernel.

90-day target:

- deterministic execution
- verified evolution loop
- stable governor control
- replay-driven improvement
- network-ready evolution node

## 2. Development Strategy

```text
Kernel First -> Evolution Second -> Network Last
```

Do not build agents or UI before kernel stability.

## 3. Phase Overview

| Phase | Duration | Focus | Outcome |
| --- | --- | --- | --- |
| Phase 0 | Week 1 | Kernel Skeleton | Compile-ready core |
| Phase 1 | Week 2-3 | Deterministic Execution | Replay-safe runtime |
| Phase 2 | Week 4-5 | Evolution Solidification | Assets generated |
| Phase 3 | Week 6-7 | Selection and Replay | Self-reuse begins |
| Phase 4 | Week 8-9 | Governor Stability | Safe evolution |
| Phase 5 | Week 10-12 | Network Foundation | Evolution sharing |

## 4. Phase 0 - Kernel Skeleton

Goals:

- establish minimal module boundaries
- define `Executor`, `Validator`, `Solidifier`, `Selector`, `EvolutionStore`

Deliverable:

- kernel compiles
- trait interfaces stable

## 5. Phase 1 - Deterministic Execution

Implement:

- step execution
- retry safety
- interrupt recovery
- trace system recording inputs, mutations, outputs, environment hash
- replay engine v0

Acceptance:

- identical replay output
- deterministic execution hash

## 6. Phase 2 - Evolution Solidification

Implement:

- codex adapter capturing patch diff, logs, validation result
- signal extraction from logs
- solidifier that emits gene, capsule, and evolution event
- append-only evolution store

Acceptance:

- successful executions generate capsules automatically

## 7. Phase 3 - Selection and Replay

Selector factors:

- success rate
- reuse count
- recency
- environment match

Replay order:

```text
Detect Signals
-> Find Capsule
-> Apply Patch
-> Validate
```

Acceptance:

- repeated issue solved without new reasoning
- token usage decreases

## 8. Phase 4 - Governor Stability Layer

Implement:

- mutation rate limit
- blast radius check
- confidence decay
- regression detection
- cooling window

Acceptance:

- harmful strategies auto-revoked
- stable success rate over time

## 9. Phase 5 - Evolution Network Foundation

Implement:

- evolution envelope
- publish API (`POST /evolution/publish`)
- fetch API (`GET /evolution/fetch`)
- quarantine system

Acceptance:

- node imports remote capsule
- local validation required
- replay succeeds from remote knowledge

## 10. Parallel Workstreams

Observability:

- replay success rate
- promotion ratio
- revoke frequency
- mutation velocity

Testing:

- deterministic replay tests
- sandbox safety tests
- governor regression tests

Documentation sync:

- architecture
- evolution
- governor
- network
- economics
- kernel

## 11. Milestones

- Milestone A, Day 30: EvoKernel alive, assets generated
- Milestone B, Day 60: self-reuse, replay replaces reasoning
- Milestone C, Day 90: distributed learning across nodes

## 12. Major Risks

| Risk | Mitigation |
| --- | --- |
| Non-deterministic execution | strict replay checks |
| Evolution spam | governor limits |
| Strategy monoculture | exploration sampling |
| Network poisoning | quarantine validation |

## 13. Recommended Team Allocation

| Role | Responsibility |
| --- | --- |
| Kernel Engineer | execution and replay |
| Evolution Engineer | solidify and selector |
| Safety Engineer | governor |
| Infra Engineer | sandbox and network |

Small teams of 2 to 4 engineers are sufficient.

## 14. Definition of Success

Oris is successful when:

- repeated failures auto-resolve
- reasoning frequency declines
- execution stabilizes
- intelligence accumulates safely

## 15. Post-Roadmap Direction

- evolution economy activation
- multi-org federation
- autonomous improvement pipelines
- enterprise deployment layer
