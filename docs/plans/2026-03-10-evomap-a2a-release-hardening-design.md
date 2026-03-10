# EvoMap A2A Release Hardening Design

Date: 2026-03-10
Status: Approved for planning

## 1. Objective

In a two-week window, raise the merged EvoMap/A2A semantic surface from
"implemented" to "release-controllable": reproducible, auditable, and blocked by
deterministic quality gates before publish approval.

## 2. Baseline

- branch: `main`
- sync status: `git pull` => `Already up to date`
- current head during design: `a37b9ed`
- semantic parity series `#148` to `#155` is merged (PR `#156` to `#164`)

## 3. Scope and Boundaries

In scope:

- harden existing `/a2a/*` semantic behavior and release readiness
- standardize evidence bundle shape for release-candidate runs
- enforce SQLite/Postgres parity checks as release blockers unless explicitly
  waived with traceable approval
- keep exactly one human gate at publish approval

Out of scope:

- adding broad new protocol families in this two-week cycle
- reducing or removing the human publish-approval gate
- introducing manual bypass scripts for release gating

## 4. Architecture and Responsibility Split

- `oris-runtime`:
  - source of truth for semantic behavior and policy outcomes
- `oris-execution-runtime`:
  - persistence semantics and backend parity behavior (SQLite/Postgres)
- `oris-orchestrator`:
  - evidence collection and release-candidate flow coordination only
  - must not override runtime policy decisions
- CI / release gate:
  - decides publish eligibility from deterministic checks only
  - must not compute business semantics

## 5. Release-Candidate Data Flow

1. semantic change enters release-candidate branch
2. validation chain runs in fixed order:
   `contract -> e2e -> backend parity -> evidence bundle`
3. one canonical run id is generated and bound to commit SHA
4. all-pass gate moves candidate to `ReleasePendingApproval`
5. explicit human approval allows publish
6. publish metadata links to immutable evidence bundle

## 6. Mandatory Gates (All Pass)

- contract suite:
  - core `/a2a/*` semantics, idempotency, and error contracts
- e2e suite:
  - lifecycle happy path plus at least two failure paths
- backend parity:
  - SQLite/Postgres produce equivalent semantic outcomes on the same cases
- evidence completeness:
  - test summary, retries/failures, parity report, approval record

## 7. Failure and Rollback Policy

- any gate failure blocks publish
- gate failures are classified and routed:
  - semantic regression -> immediate fix issue
  - backend mismatch -> `parity-blocker` priority
  - missing evidence -> fail closed (no verbal override)
- rollback target is candidate code or missing evidence only
- gating rules themselves are not rolled back during incident handling

## 8. Test Matrix and Acceptance Criteria

Contract layer:

- endpoint contract, deterministic errors, idempotent behavior for key routes

E2E layer:

- task lifecycle primary path
- at least two exceptional paths (for example policy reject and validation
  failure)

Backend parity layer:

- same case set executed on SQLite and Postgres
- compare state transitions, key fields, and audit traces

Release-gate layer:

- evidence bundle completeness and state-transition correctness

Definition of done for this cycle:

- at least one full candidate run passes the full gate chain reproducibly
- no untracked backend semantic drift remains
- every failed run can be diagnosed from evidence bundle artifacts
- publish approval record and evidence bundle map to exactly one commit

## 9. Two-Week Milestones

Week 1 (2026-03-10 to 2026-03-16):

- normalize gate entrypoints and evidence schema
- establish backend parity baseline and blocker taxonomy

Week 2 (2026-03-17 to 2026-03-23):

- clear blocking parity/regression issues
- complete release-candidate rehearsal and go/no-go report

## 10. Decision Record

Chosen option: stability-first hardening (no major semantic expansion in this
window).

Rationale:

- best fit for "release controllable first" objective
- minimizes risk concentration in a short cycle
- yields explicit publish criteria for subsequent semantic expansion cycles
