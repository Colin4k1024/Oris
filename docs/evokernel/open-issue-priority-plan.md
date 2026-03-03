# EvoKernel Open Issue Priority Plan

Last synced: March 3, 2026

## Objective

Close out stale EvoKernel issues that already appear implemented, then execute the remaining backlog in the smallest release-safe order.

This plan is based on:

- the current open GitHub issue list
- code present on `main`
- the existing EvoKernel roadmap and release flow

## Current Open Issues

| Issue | Title | Working Classification | Recommended Action |
| --- | --- | --- | --- |
| #79 | Documentation Sync | likely mostly complete | final review, then close |
| #78 | Bootstrap and Initial Seeding | not started | implement later |
| #77 | Multi-Agent Coordination | partially enabled by Wave 7 | implement after core safety gaps |
| #76 | Governor Detailed Policies | partially implemented | implement next after testing |
| #75 | Evolution Testing Suite | partially implemented | highest active development priority |
| #72 | Evolution Solidification Engine | incomplete | implement after testing and governor |
| #67 | Asset lifecycle states and event extensions | likely already complete | verify on `main`, then close |
| #66 | OEN protocol and quarantine import pipeline | likely already complete | verify on `main`, then close |

## Phase 1: Close Out Likely-Completed Issues

These issues should be handled before new feature work when their acceptance criteria already appear satisfied by code currently on `main`.

### 1. Issue #66

Why first:

- it is the oldest actionable open issue
- the core OEN types and import/export path already exist
- closing it reduces backlog noise before new work starts

Verification focus:

- confirm `EvolutionEnvelope` defaults and canonical hashing
- confirm `export_promoted_assets(...)` and `import_remote_envelope(...)`
- confirm runtime/server experimental publish, fetch, and import routes still compile and behave as expected

Closeout rule:

- if acceptance criteria match current behavior, close without new code
- if there is a narrow missing acceptance item, implement only that gap and release

### 2. Issue #67

Why second:

- it is also an old issue and likely already shipped implicitly
- lifecycle state support is a dependency foundation for later waves

Verification focus:

- `AssetState` and lifecycle transitions
- `Outcome.lines_changed` and `Outcome.replay_verified`
- blast radius support from sandbox patch parsing
- selector defaulting to `Promoted`
- legacy event compatibility

Closeout rule:

- if current `main` satisfies the acceptance criteria, close without adding more code
- if a compatibility or projection gap remains, fix only that gap

### 3. Issue #79

Why third:

- the documentation was recently updated and likely needs only final confirmation
- it is lower engineering risk than new feature implementation

Verification focus:

- all EvoKernel design documents have current status markers
- API examples reflect current code
- cross-reference drift is reduced to an acceptable level

Closeout rule:

- close after a final document pass confirms the docs are aligned enough for the current implementation snapshot

## Phase 2: Active Development Order

After the stale open issues are resolved, the remaining backlog should be implemented in this order.

### 1. Issue #75 - Evolution Testing Suite

Why first:

- smallest high-value surface area
- improves confidence for every later EvoKernel release
- supports safer work on governor and solidification behavior

Target scope:

- deterministic replay regression coverage
- sandbox safety regression coverage
- governor regression coverage
- at least one clear end-to-end replay lifecycle test

Release expectation:

- patch release unless public runtime API changes are needed

### 2. Issue #76 - Governor Detailed Policies

Why second:

- directly extends already-shipped governor behavior
- safety work should land before more autonomous orchestration

Target scope:

- mutation rate limits
- per-window or per-capsule limits
- confidence decay
- stronger cooling and regression handling

Release expectation:

- patch or minor depending on whether runtime-facing configuration expands

### 3. Issue #72 - Evolution Solidification Engine

Why third:

- foundational, but larger than testing or governor refinement
- easier to validate after the testing suite is strengthened

Target scope:

- codex adapter inputs
- deterministic signal extraction and normalization
- explicit solidifier path
- append-only evolution store guarantees

Release expectation:

- likely minor because it deepens the evolution pipeline significantly

### 4. Issue #77 - Multi-Agent Coordination

Why fourth:

- depends on the Wave 7 contract work already shipped
- should build on a more stable solidification and governor baseline

Target scope:

- planner, coding, repair, and optimization role coordination
- shared capture pipeline across roles
- replay-aware recurring task flow with reduced repeated reasoning

Release expectation:

- minor

### 5. Issue #78 - Bootstrap and Initial Seeding

Why last:

- it is the most autonomous behavior in the remaining backlog
- it should be layered on top of stable solidification, replay, and governor mechanics

Target scope:

- first-run empty-store detection
- seed gene and capsule initialization
- validation of seed quality
- early self-improvement loop bootstrapping

Release expectation:

- minor or larger depending on how much runtime automation is introduced

## Operating Rules For This Backlog

- Prefer closing already-satisfied issues before opening new implementation branches.
- Keep each issue to one releaseable unit.
- Run targeted EvoKernel tests first, then broader workspace validation, before any publish.
- Do not start `#77` or `#78` until `#75` and `#76` have reduced the safety risk.

## Immediate Next Step

Start with a narrow audit branch for `#66`, prove whether the current `main` already satisfies its acceptance criteria, and either:

- close it directly, or
- land the smallest missing gap and release only that delta
