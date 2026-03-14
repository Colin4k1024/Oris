# W8 Self-Evolution Roadmap Design

> Archive note: This document is retained as historical W8 planning context.
> The planned issue set `#234` through `#238` has since been implemented and merged to `main`.
> Current source-of-truth status lives in `docs/evokernel/self-evolution-acceptance-checklist.md` and the checked-in code/tests.

Date: 2026-03-14
Status: Archived planning record
Scope: self-evolution roadmap only

## Context

W7 completed the replay-memory hardening track for self-evolution:

- task-class generalization shipped
- confidence control shipped
- replay ROI and release-gate stability shipped
- supervised devloop expansion shipped
- federated attribution and revocation hardening shipped

The current product boundary in [docs/evokernel/self-evolution-acceptance-checklist.md](/Users/jiafan/Desktop/poc/Oris/docs/evokernel/self-evolution-acceptance-checklist.md) still describes Oris as:

> constrained replay-driven self-evolution

W8 should move the system one stage forward without overclaiming autonomy. The user explicitly asked that W8 be split strictly along the self-evolution line, without mixing in broader kernel, observability, or generic platform work.

## Goal

Define a W8 issue set that advances Oris from replay-driven reuse toward supervised closed-loop self-evolution.

## Non-Goals

W8 does not include:

- autonomous merge or autonomous release
- generic kernel hardening unrelated to self-evolution
- broad observability work that is not required by the self-evolution loop
- unconstrained codebase-wide mutation orchestration
- fully autonomous issue intake outside bounded self-evolution scenarios

## Recommended Split Strategy

Three split strategies were considered:

1. Autonomy-level split
2. Closed-loop stage split
3. Cross-cutting capability split

The recommended strategy is the closed-loop stage split.

Rationale:

- It maps directly to the missing stages in the current acceptance checklist.
- It keeps each issue small enough to ship as one releasable slice.
- It preserves a clear machine-readable audit trail from intake to delivery.
- It reduces the risk of turning one issue into an architecture-only refactor.

## W8 Target Statement

After W8, the intended product statement becomes:

> Oris supports supervised closed-loop self-evolution for a bounded subset of development work.

This remains narrower than autonomous self-development.

The new boundary should mean:

- the runtime can select a bounded self-evolution candidate
- the runtime can generate a structured mutation proposal
- proposal execution can reuse replay knowledge and fail closed on policy or validation problems
- the system can prepare branch and PR delivery evidence under supervision
- acceptance and audit outputs remain machine-readable and consistent across API, events, and tests

## Architectural Shape

W8 should continue to reuse the existing self-evolution architecture instead of introducing a new subsystem.

Primary components:

- [crates/oris-agent-contract/src/lib.rs](/Users/jiafan/Desktop/poc/Oris/crates/oris-agent-contract/src/lib.rs)
- [crates/oris-evokernel/src/core.rs](/Users/jiafan/Desktop/poc/Oris/crates/oris-evokernel/src/core.rs)
- [crates/oris-runtime/tests/evolution_feature_wiring.rs](/Users/jiafan/Desktop/poc/Oris/crates/oris-runtime/tests/evolution_feature_wiring.rs)
- [crates/oris-runtime/tests/agent_self_evolution_travel_network.rs](/Users/jiafan/Desktop/poc/Oris/crates/oris-runtime/tests/agent_self_evolution_travel_network.rs)
- [docs/evokernel/self-evolution-acceptance-checklist.md](/Users/jiafan/Desktop/poc/Oris/docs/evokernel/self-evolution-acceptance-checklist.md)

Core rule:

- extend the existing contract and event vocabulary where possible
- keep fail-closed behavior explicit
- require machine-readable evidence at every stage

## Shared Closed-Loop Data Flow

All W8 issues should align to one explicit, auditable flow:

1. Intake
- ingest an issue or backlog candidate
- produce a machine-readable selection decision
- reject out-of-scope work with fail-closed policy output

2. Planning
- generate a structured mutation proposal
- declare file scope, task class, validation budget, approval needs, and expected evidence
- reject malformed or out-of-bounds proposals before execution

3. Execution
- route approved proposals into the existing supervised devloop and replay-assisted paths
- keep replay hit, fallback reason, validation result, and confidence outcomes in one execution contract

4. Delivery
- produce a bounded branch and PR preparation summary
- do not merge or release automatically

5. Audit gate
- aggregate reason code, approval state, evidence summary, and delivery outcome
- stop the loop if any stage lacks required evidence or violates policy

## Error Handling Principles

W8 should keep the W7 fail-closed style rather than inventing a separate error universe.

Default mappings:

- out-of-scope candidate or budget overrun: `PolicyDenied`
- malformed proposal or missing required delivery evidence: `ValidationFailed`
- patch boundary or path escape: `UnsafePatch`
- missing approval or stage timeout: `Timeout`
- unattributable critical evidence: fail closed without partial forward progress

Rules:

- no silent partial success
- no best-effort continuation after missing critical evidence
- API contract, event stream, and tests should share the same reason-code meaning

## Testing Strategy

Each W8 issue should include three validation layers.

1. EvoKernel regression
- positive-path stage behavior
- fail-closed negative control
- reason code and evidence consistency assertions

2. Runtime wiring
- facade exposure
- feature-flag wiring
- machine-readable contract stability

3. Storyline end-to-end coverage
- candidate selection to proposal to execution to delivery summary
- at least one success path
- at least one fail-closed policy or approval path

## Proposed W8 Issue Set

### #234 Candidate Intake and Selection Contracts

Title:
`[EVO26-W8-01][P1] Self-Evolution Candidate Intake and Selection Contracts`

Intent:
- select bounded self-evolution work items from issue or backlog inputs
- emit machine-readable accept or reject decisions with policy reasons

Why this comes first:
- later stages should not invent their own candidate boundary logic

### #235 Structured Mutation Proposal Contracts

Title:
`[EVO26-W8-02][P1] Structured Mutation Proposal Contracts for Supervised Evolution`

Intent:
- transform an accepted candidate into a machine-readable proposal contract
- encode file scope, task class, budgets, and expected validation evidence before execution

Why this is second:
- execution should consume a declared proposal instead of ad hoc request shapes

### #236 Replay-Assisted Supervised Execution Loop

Title:
`[EVO26-W8-03][P1] Replay-Assisted Supervised Execution Loop`

Intent:
- route approved proposals through replay-aware supervised execution
- unify replay hints, fallback reasons, execution evidence, and failure contracts

Why this is third:
- it is the first issue that turns intake and proposal contracts into a real runtime-owned loop

### #237 Bounded Branch and PR Delivery

Title:
`[EVO26-W8-04][P1] Bounded Branch and PR Delivery for Self-Evolution`

Intent:
- prepare branch and pull-request delivery artifacts under supervision
- keep branch and PR behavior bounded, auditable, and non-autonomous

Why this is fourth:
- it depends on structured proposal and execution evidence already existing

### #238 Closed-Loop Acceptance and Audit Consistency

Title:
`[EVO26-W8-05][P1] Closed-Loop Acceptance Gate and Audit Consistency`

Intent:
- unify the W8 loop into one acceptance and audit gate
- ensure reason code, approval evidence, and delivery outcome stay consistent across API, events, and tests

Why this is last:
- it should consolidate the contracts created by the earlier four issues rather than forcing them prematurely

## Sequencing and Release Shape

Recommended execution order:

1. W8-01 candidate intake and selection
2. W8-02 structured mutation proposals
3. W8-03 replay-assisted supervised execution
4. W8-04 bounded branch and PR delivery
5. W8-05 acceptance gate and audit consistency

Each issue should remain releasable on its own.

## Definition of Done for the W8 Track

W8 is complete when:

- all five W8 issues ship and close
- the self-evolution acceptance checklist is updated to the new supervised closed-loop boundary
- machine-readable contracts cover candidate selection, proposal generation, execution, delivery, and audit gate outputs
- fail-closed reason-code behavior remains consistent across API, events, and tests
- Oris can prepare a bounded self-evolution change from candidate selection through supervised delivery evidence without claiming autonomous release
