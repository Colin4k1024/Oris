# Evo A2A Semi-Autonomous Release Design

Date: 2026-03-05
Status: Approved for planning

## 1. Objective

Build a semi-autonomous evolution workflow where Oris can execute issue-to-merge automatically using A2A coordination, while keeping exactly one human gate before release publish.

Target mode:

- automation level: B (semi-autonomous)
- release model: publish-gated by one explicit human approval
- non-goal: fully autonomous end-to-end release without human confirmation

## 2. Current Baseline (Repository Ground Truth)

Already implemented:

- A2A protocol scaffold and handshake contract in `crates/oris-agent-contract`
- runtime A2A routes for handshake, remote task sessions, snapshot, lifecycle query, and session replication
- negotiated capability and privilege enforcement for evolution routes
- handshake session persistence and restart reuse with `sqlite-persistence`
- replay feedback (`SkipPlanner` or `PlanFallback`) surfaced to callers
- supervised devloop path with explicit approval handling

Current hard boundary:

- supervised devloop is constrained to one bounded task class: single `docs/*.md` file
- no autonomous issue intake, no full planner loop, no autonomous branch or PR orchestration, and no autonomous release orchestration in checked-in runtime

## 3. Scope

In scope:

- add an external orchestrator layer that drives issue intake, planning, A2A execution, validation, branch and PR automation, merge automation, and release candidate generation
- keep Evo runtime as execution and policy engine
- keep one human approval gate only at release publish time
- preserve deterministic audit evidence across every stage

Out of scope:

- removing the final human release gate
- replacing A2A with an ad hoc protocol
- broadening runtime internal responsibilities into a monolithic orchestrator

## 4. Architecture

### 4.1 Components

- `Orchestrator Service`:
  - owns intake queue, task planning, run coordination, and GitHub workflow automation
- `Evo Runtime`:
  - owns replay-first execution, mutation capture, validation, governor decisions, lifecycle recording
- `A2A Control Plane`:
  - owns protocol handshake, capability negotiation, session lifecycle, and cross-node session replication
- `GitHub Adapter`:
  - owns issue reads, branch creation, PR creation, status updates, and merge calls
- `Release Gate Adapter`:
  - owns one explicit publish approval checkpoint and execution of publish actions after approval
- `Evidence Store`:
  - stores task spec, replay feedback, validation artifacts, policy decisions, PR metadata, and release evidence bundle

### 4.2 Responsibility Split

- runtime must not decide repository release orchestration policy
- orchestrator must not bypass runtime policy and validation outcomes
- release adapter must only execute publish after explicit approved state

## 5. End-to-End Data Flow

Primary flow:

`Issue -> Plan -> A2A Session -> Replay or Fallback -> Patch -> Validate -> PR -> Merge -> Release Pending Approval -> Publish -> Capture`

Stage details:

1. Intake: orchestrator fetches and normalizes issue into `TaskSpec`
2. Session bootstrap: orchestrator negotiates A2A handshake and starts remote task session
3. Execution: runtime applies replay-first decision and fallback planning if needed
4. Validation: tests and policy gates run before any PR creation
5. PR: branch and PR are created with deterministic evidence and risk summary
6. Merge: auto-merge only when required checks are green
7. Release gate: only human checkpoint, then automated publish and release-note finalize
8. Capture: successful outcomes are persisted as evolution assets and linked to issue lifecycle

## 6. Task State Machine

Task state machine:

- `Queued`
- `Planned`
- `Dispatched`
- `InProgress`
- `Validated`
- `PRReady`
- `Merged`
- `ReleasePendingApproval`
- `Released`

Failure states:

- `FailedRetryable`
- `FailedTerminal`
- `Cancelled`

Transition rules:

- no transition to `PRReady` without passing validation gate
- no transition to `Released` without explicit approval record
- terminal failures must include deterministic fallback reason and evidence references

## 7. Safety and Governance Model

Automated gates:

- validation gate: compile, test, targeted regression, and policy checks
- capability gate: A2A negotiated capability + privilege profile required per action
- scope gate: patch must stay inside task allowed paths
- PR quality gate: evidence summary and rollback plan required

Human gate:

- exactly one gate at release publish
- decision options: approve publish or reject publish
- reject keeps state at `ReleasePendingApproval` and opens remediation task

Audit requirements:

- every gate decision must be persisted with actor, reason, request id, and timestamps
- release must include immutable evidence bundle id

## 8. Milestones and Acceptance

Phase 0 (2026-03-06 to 2026-03-08): design freeze

- acceptance: interfaces and state machine stable, no unresolved blockers

Phase 1 (2026-03-09 to 2026-03-20): MVP closed loop to merge

- acceptance: at least 5 issues complete issue-to-merge with no manual steps before release gate

Phase 2 (2026-03-21 to 2026-04-03): release gate integration

- acceptance: at least 2 successful supervised publishes, each with approval and evidence

Phase 3 (2026-04-04 to 2026-04-17): reliability hardening

- acceptance: >= 95% success in fault-injection runs, no policy bypass

## 9. Risks and Mitigations

Risk: orchestrator bypasses runtime policy

- mitigation: runtime remains the only source for execution accept or reject decisions

Risk: session drift across nodes

- mitigation: enforce protocol version checks and session replication validation

Risk: over-automation before release

- mitigation: keep immutable release approval checkpoint with auditable actor identity

Risk: evidence inconsistency

- mitigation: evidence bundle generated from one canonical run id and attached to PR and release

## 10. Decision Record

Chosen approach: external orchestrator first.

Reason:

- fastest path to target capability with minimal invasive runtime changes
- preserves current A2A and Evo boundaries
- allows incremental rollout and rollback at orchestrator layer
