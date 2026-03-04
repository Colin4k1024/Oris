# EvoKernel Open Issue Execution Queue

Last synced: March 3, 2026

This document replaces the earlier "likely complete" backlog note with an
evidence-based execution queue. It is grounded in the current open GitHub issue
list, code present on `main`, the checked-in EvoKernel design docs, and the
Oris issue-driven release workflow.

Working defaults:

- audit-only closures and repo-doc-only closures do not require an
  `oris-runtime` publish
- every code-changing issue remains one releasable unit
- any issue that would require a breaking API, config, persistence, or behavior
  change must stop and escalate instead of selecting a major release implicitly

## Ground Truth Snapshot (March 3, 2026)

| Issue | Current State | Evidence-Based Status |
| --- | --- | --- |
| #66 | mostly implemented | `oris-evolution-network` already exposes `EvolutionEnvelope`, OEN DTOs, content-hash verification, `export_promoted_assets(...)`, `import_remote_envelope(...)`, and runtime publish/fetch/revoke routes with route tests. |
| #67 | mostly implemented | `AssetState`, `Outcome.lines_changed`, `Outcome.replay_verified`, quarantine/spec-link events, projection rebuild logic, and sandbox blast-radius parsing already exist in the checked-in crates. |
| #79 | mostly implemented | the tracked EvoKernel docs already contain implementation status markers; the remaining work is cross-reference drift and API wording confirmation. |
| #75 | partially implemented | EvoKernel already has inline async tests, sandbox tests, and experimental feature wiring, but it does not yet expose a clearly separated regression suite that matches the issue acceptance criteria. |
| #76 | partially implemented | the governor already supports success-threshold promotion, blast-radius caps, replay-failure revocation, time-window mutation rate limiting, confidence-regression checks, and a promotion cooling window, but it still lacks concurrent mutation limits and a richer confidence-history model. |
| #72 | partially implemented | the evolution stack already has append-only storage, selector ranking, mutation capture, replay-first reuse, and environment-aware matching, but it still lacks a dedicated signal-extraction stage and clearer solidification boundaries. |
| #77 | early-stage | the repository already contains the agent-contract scaffold and proposal path, but it does not yet contain a real coordination runtime. |
| #78 | not started | `bootstrap.md` explicitly states there is no dedicated bootstrap subsystem yet, so this remains the final autonomy issue. |

## Immediate Audit Closeouts

These issues should be resolved before new feature work. They are handled as
evidence-backed audits first, not as assumed code changes.

### 1. Issue #66 - OEN protocol and quarantine import pipeline

Classification:

- `audit-first`

Audit evidence:

- `oris-evolution-network` already defines the OEN contract area in
  `crates/oris-evolution-network/src/lib.rs`
- `oris-evokernel` already exposes `export_promoted_assets(...)` and
  `import_remote_envelope(...)`
- `oris-runtime` already exposes experimental publish, fetch, and revoke routes
  in `crates/oris-runtime/src/execution_server/api_handlers.rs`
- route tests already exist for publish/fetch/revoke behavior

Audit scope:

- verify `EvolutionEnvelope` defaults and content-hash behavior
- verify `export_promoted_assets(...)` and `import_remote_envelope(...)`
- verify remote assets remain quarantined until local validation completes
- verify the existing publish/fetch/revoke route behavior still matches the issue

Public surface rule:

- if the audit passes, do not add new public surface
- if a gap exists, the only allowed surface expansion is inside the already
  declared OEN contract area and the existing experimental runtime routes

Closeout rule:

- start on `main` with no branch
- create `codex/issue-66-oen-gap` only if the audit finds a real missing
  acceptance item
- if the audit passes, move the issue to `in progress`, post an evidence-only
  closeout comment, and close it with no version bump and no publish
- if the audit fails, implement only the missing acceptance item and treat that
  follow-up as a `feature` with a `minor` release

Required verification before closeout:

- code-reference verification
- at least one targeted existing test covering export/import or the runtime
  route flow

Key test cases:

- export promoted assets
- reject bad hashes
- keep remote imports quarantined until local validation
- verify publish/fetch/revoke route behavior

### 2. Issue #67 - asset lifecycle states and event extensions

Classification:

- `audit-first`

Audit evidence:

- `crates/oris-evolution/src/core.rs` already defines `AssetState` and extends
  `Outcome` with `lines_changed` and `replay_verified`
- `crates/oris-evokernel/src/core.rs` already emits quarantine and spec-link
  events and uses lifecycle-aware logic
- `crates/oris-sandbox/src/core.rs` already computes blast radius from patch
  text

Audit scope:

- verify lifecycle types and transitions
- verify projection rebuild behavior
- verify legacy log compatibility
- verify selector defaults to promoted assets by default
- verify blast-radius data reaches stored outcomes

Public surface rule:

- this is expected to be a no-new-surface closeout
- if a gap exists, only compatibility or projection fixes to existing evolution
  types are in scope

Closeout rule:

- start on `main` with no branch
- create `codex/issue-67-lifecycle-gap` only if the audit finds a missing
  compatibility or projection item
- if the audit passes, close with no release
- if the audit fails, fix only the missing compatibility or projection defect
  and keep the release at `patch` unless the gap unexpectedly expands a public
  type

Required verification before closeout:

- code-reference verification
- at least one targeted existing test covering projection rebuild, lifecycle
  state, or compatibility behavior

Key test cases:

- rebuild projection from historical events
- preserve legacy log compatibility
- ensure only promoted assets are selected by default
- confirm blast-radius data reaches outcomes

### 3. Issue #79 - documentation sync

Classification:

- `doc-audit`

Audit evidence:

- the tracked EvoKernel docs already contain implementation status markers
- the remaining gap is cross-reference drift, API example drift, and status
  wording confirmation

Audit scope:

- review the tracked EvoKernel docs listed in the issue
- confirm implementation status markers remain present and accurate
- confirm no major API example contradicts the current code
- reduce cross-reference drift to an acceptable level for the current snapshot

Closeout rule:

- keep this after `#66` and `#67` so the doc pass can reflect their final state
- treat the status markers as already done work
- close with no crate publish if the work is only repo docs

Required verification before closeout:

- a sweep across the tracked EvoKernel docs confirming status markers, current
  API wording, and acceptable cross-reference alignment

Key test cases:

- every tracked EvoKernel doc includes an implementation status marker
- no major API example contradicts current code

## Active Engineering Queue

These are the first code-changing issues after the audit queue. Each issue stays
within one releasable unit.

### 1. Issue #75 - Evolution Testing Suite

Role in queue:

- first code-changing issue

Working interpretation:

- add missing black-box regression coverage around shipped EvoKernel behavior
- do not treat this as a greenfield test framework build

Scope:

- dedicated replay determinism regression coverage
- dedicated sandbox safety boundary coverage
- dedicated governor regression coverage
- one clear end-to-end replay lifecycle path

Public surface rule:

- this is test-only work and should not change public APIs

Release class:

- default `patch`

Stop conditions:

- do not expand runtime behavior as part of this issue
- if a new test exposes a product bug, fix only the smallest bug required to
  make the intended regression pass

Validation floor:

- targeted new regression tests first
- `cargo test --workspace`
- the normal pre-release baseline

Key test cases:

- replay determinism across repeated identical inputs
- sandbox denial paths
- governor threshold regressions
- one full replay lifecycle happy path

### 2. Issue #76 - Governor Detailed Policies

Role in queue:

- second code-changing issue
- must land before `#72`

Working interpretation:

- extend the existing narrow governor
- do not treat this as initial governor introduction

Scope:

- time-window mutation rate limits
- confidence decay and confidence history
- stronger regression handling
- keep existing blast-radius and cooling semantics coherent while extending them

Public surface rule:

- this is the first likely public-surface expansion after the audit queue
- changes may touch `GovernorConfig`, `GovernorInput`, or runtime-facing policy
  controls in `crates/oris-governor/src/lib.rs`

Release class:

- default `patch`
- escalate to `minor` if config or runtime-facing controls expand

Stop conditions:

- do not add global orchestration or multi-agent pause controls here
- if the required change would break existing config semantics, stop and
  escalate instead of broadening the issue

Validation floor:

- default to the `feature` validation floor unless the implementation is
  deliberately constrained to a narrower `bugfix`

Key test cases:

- rate-limit threshold hit
- blast-radius threshold hit
- confidence decay over time
- regression-triggered revocation
- cooling-window enforcement

### 3. Issue #72 - Evolution Solidification Engine

Role in queue:

- third code-changing issue
- may not start until `#75` and `#76` are both closed

Working interpretation:

- close the remaining solidification gaps on top of an already existing
  evolution core

Scope:

- dedicated signal extraction
- clearer solidification-stage boundaries
- remaining immutable query-path gaps tied to signal lookup and capsule
  creation

Public surface rule:

- this issue may expand EvoKernel query or capture interfaces, but only inside
  the missing solidification gap already called out in `evolution.md`

Release class:

- default `minor`

Stop conditions:

- do not reopen lifecycle or network work already covered by `#66` and `#67`
- if the work turns into a broad pipeline redesign, split that redesign out and
  keep this issue to the smallest releasable slice

Validation floor:

- default to the `feature` validation floor unless the implementation is
  deliberately constrained to a narrower `bugfix`

Key test cases:

- deterministic signal extraction from the same input
- immutable append semantics
- query-by-signal retrieval
- automatic capsule creation after validated execution

### 4. Issue #77 - Multi-Agent Coordination

Role in queue:

- fourth code-changing issue
- may not start until `#72` is closed

Working interpretation:

- build a new orchestration layer on top of the current proposal-only agent
  contract

Scope:

- coordination protocol
- task distribution
- failure handling

Public surface rule:

- if this changes runtime defaults, it must be feature-gated or opt-in

Release class:

- default `minor`

Stop conditions:

- do not fold bootstrap work into this issue
- do not fold transport-security work into this issue
- if transport, auth, or execution-privilege enforcement becomes a prerequisite,
  mark the issue `blocked` instead of absorbing that work here

Validation floor:

- default to the `feature` validation floor unless the implementation is
  deliberately constrained to a narrower `bugfix`

Key test cases:

- planner-to-worker handoff
- partial failure recovery
- timeout and retry
- deterministic result aggregation

### 5. Issue #78 - Bootstrap and Initial Seeding

Role in queue:

- fifth code-changing issue
- may not start until `#77` is closed

Working interpretation:

- deliver the final autonomy issue after the rest of the replay, governance, and
  orchestration stack is stable

Scope:

- empty-store detection
- first-run seed injection
- seed validation
- controlled transition into seeded replay catalogs

Public surface rule:

- if this changes runtime startup defaults, it must be feature-gated or opt-in

Release class:

- default `minor`

Stop conditions:

- do not plan a major release implicitly here
- if the design would break compatibility, stop and escalate instead of forcing
  it into this issue

Validation floor:

- default to the `feature` validation floor unless the implementation is
  deliberately constrained to a narrower `bugfix`

Key test cases:

- empty-store detection
- first-run seed injection
- seed validation failure handling
- no reseeding on already initialized stores

## Per-Issue Operating Contract

Apply these rules to every issue in this queue unless the issue text explicitly
narrows them further.

### Branch rule

- audit-first and doc-audit issues begin on `main` with no new branch
- create an issue branch only when the audit finds a real code gap
- use the `codex/issue-<number>-<slug>` pattern for issue branches

### Issue comment status rule

- track issue state in GitHub comments, not labels, unless status labels are
  introduced later
- use one visible status at a time: `in progress`, `blocked`, or `released`
- post `in progress` immediately when the issue becomes the active work item
- post `blocked` as soon as a missing service, secret, dependency, or decision
  prevents completion
- post `released` only after `cargo publish` succeeds

### Validation floor

- `#66` and `#67` require code-reference verification plus at least one targeted
  existing test before closeout
- `#79` requires a doc sweep confirming status markers, current API wording, and
  acceptable cross-reference alignment
- `#75` requires feature-grade validation breadth even though the shipped impact
  is test-only
- `#76`, `#72`, `#77`, and `#78` default to the `feature` validation floor
  unless they are deliberately constrained to a narrower `bugfix`
- run the matching CI-aligned regression commands whenever an issue touches
  execution-server security or runtime persistence

### Publish rule

- audit-only closures and repo-doc-only closures do not require an
  `oris-runtime` publish
- no code-changing issue closes until the chosen version bump, release note,
  publish result, and issue comment all agree
- use the issue type default as the starting release class, then escalate only
  when the shipped impact is higher but still backward-compatible

## Blocked-Until Rules

- do not start `#72` until both `#75` and `#76` are closed
- do not start `#77` until `#72` is closed
- do not start `#78` until `#77` is closed
- if an earlier issue expands beyond its smallest releasable slice, split the
  extra work into a follow-up issue instead of violating the queue

## Immediate Next Step

Start with `#66` as an audit on `main`, verify the existing OEN behavior against
its acceptance criteria, and either close it without a release or open a narrow
issue branch only for the smallest missing acceptance item.
