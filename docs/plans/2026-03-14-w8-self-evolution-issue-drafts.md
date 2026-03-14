# W8 Self-Evolution Issue Drafts

> Archive note: These drafts are preserved as historical planning artifacts.
> The corresponding W8 issues `#234` through `#238` have already been created, completed, and merged to `main`.
> This file should be read as issue-planning history rather than an active backlog.

This document contains GitHub-ready draft issues for the next self-evolution track after W7.

Recommended labels for all issues:
- `type/feature`
- `priority/P1`
- `area/evolution`
- `plan`

Recommended execution order:
1. `#234` / W8-01
2. `#235` / W8-02
3. `#236` / W8-03
4. `#237` / W8-04
5. `#238` / W8-05

---

## Issue Draft 1 (`#234`)

### Title

`[EVO26-W8-01][P1] Self-Evolution Candidate Intake and Selection Contracts`

### Body

## Why
W7 hardened replay memory, supervised devloop bounds, and federated revocation, but Oris still depends on an external caller to decide which work items are valid self-evolution candidates.

To move toward supervised closed-loop self-evolution, the runtime needs a bounded intake and selection step that can classify issue or backlog items, reject out-of-scope work fail-closed, and emit machine-readable selection evidence for downstream planning.

## Scope
- define a machine-readable candidate intake request and selection decision contract
- classify whether a candidate is eligible for bounded self-evolution handling
- encode rejection reasons and recovery hints for out-of-scope or over-budget candidates
- emit stable event and API evidence for accepted and rejected candidate decisions
- keep the boundary explicitly limited to supervised self-evolution work items

## Definition of Done
- a runtime-owned selection path can accept a bounded issue or backlog candidate input and return a machine-readable decision
- rejected candidates fail closed with consistent `reason_code`, `failure_reason`, and `recovery_hint`
- selection evidence is represented consistently across API contract, event stream, and regression tests
- runtime wiring exposes the new selection surface under the existing evolution feature boundary

## Non-goals
- no mutation proposal generation yet
- no execution or branch orchestration yet
- no autonomous issue discovery from external systems without explicit input
- no expansion into non-self-evolution candidate classes

## Required Machine-Readable Outputs
- `selection_decision`
- `candidate_class`
- `reason_code`
- `recovery_hint`
- `fail_closed`

## Minimum Validation
- `cargo test -p oris-evokernel --test evolution_lifecycle_regression candidate_intake_ -- --nocapture`
- `cargo test -p oris-runtime --test evolution_feature_wiring --features full-evolution-experimental -- --nocapture`
- `cargo test --workspace`

---

## Issue Draft 2 (`#235`)

### Title

`[EVO26-W8-02][P1] Structured Mutation Proposal Contracts for Supervised Evolution`

### Body

## Why
After a candidate is selected, the next missing boundary is a structured proposal contract. Today, execution inputs are still too caller-shaped. W8 needs a declared mutation proposal that states file scope, task class, validation budget, approval requirements, and expected evidence before the runtime enters execution.

This keeps self-evolution bounded, auditable, and fail-closed.

## Scope
- define a machine-readable mutation proposal contract for approved self-evolution candidates
- require proposals to declare target files, bounded task class, validation budget, and expected evidence
- reject malformed or out-of-bounds proposals before any execution begins
- keep proposal failure contracts aligned with existing fail-closed reason-code semantics
- surface proposal contracts through runtime-facing evolution APIs or facades

## Definition of Done
- an accepted candidate can be transformed into a machine-readable mutation proposal
- proposal validation rejects missing required fields, out-of-bounds paths, or unsupported task classes before execution
- proposal approval or rejection evidence stays consistent across events, contracts, and tests
- the runtime facade exposes the proposal contract shape under the experimental evolution surface

## Non-goals
- no real mutation execution yet
- no branch or PR creation yet
- no widening beyond bounded supervised self-evolution tasks
- no autonomous approval bypass

## Required Machine-Readable Outputs
- `mutation_proposal`
- `proposal_scope`
- `validation_budget`
- `approval_required`
- `expected_evidence`
- `reason_code`
- `fail_closed`

## Minimum Validation
- `cargo test -p oris-evokernel --test evolution_lifecycle_regression mutation_proposal_ -- --nocapture`
- `cargo test -p oris-runtime --test evolution_feature_wiring --features full-evolution-experimental -- --nocapture`
- `cargo test --workspace`

---

## Issue Draft 3 (`#236`)

### Title

`[EVO26-W8-03][P1] Replay-Assisted Supervised Execution Loop`

### Body

## Why
W7 proved replay reuse and bounded supervised devloop pieces independently, but W8 still needs a runtime-owned path that takes an approved proposal, applies replay hints when available, falls back safely when not, and records execution evidence in one coherent contract.

This issue is the first true closed-loop execution slice in the W8 track.

## Scope
- connect approved mutation proposals to a replay-aware supervised execution path
- unify replay hit, fallback reason, validation result, and execution evidence in one machine-readable outcome
- ensure policy-denied, validation-failed, unsafe-patch, and timeout outcomes remain fail-closed
- preserve confidence and reason-code consistency across replay-assisted execution outcomes
- extend runtime-facing storyline coverage for proposal to execution transitions

## Definition of Done
- approved proposals can execute through one replay-aware supervised path
- replay hits and fallback outcomes are represented in a unified execution contract
- failed execution states stop the loop cleanly with machine-readable recovery output
- regression tests prove one success path and one fail-closed path for replay-assisted supervised execution

## Non-goals
- no branch or PR preparation yet
- no autonomous merge or release
- no widening into generic runtime task orchestration
- no hidden best-effort continuation after failed validation

## Required Machine-Readable Outputs
- `execution_decision`
- `replay_outcome`
- `fallback_reason`
- `validation_outcome`
- `evidence_summary`
- `reason_code`
- `recovery_hint`

## Minimum Validation
- `cargo test -p oris-evokernel --test evolution_lifecycle_regression replay_supervised_execution_ -- --nocapture`
- `cargo test -p oris-runtime --test evolution_feature_wiring --features full-evolution-experimental -- --nocapture`
- `cargo test -p oris-runtime --test agent_self_evolution_travel_network --features full-evolution-experimental -- --nocapture`
- `cargo test --workspace`

---

## Issue Draft 4 (`#237`)

### Title

`[EVO26-W8-04][P1] Bounded Branch and PR Delivery for Self-Evolution`

### Body

## Why
A supervised closed-loop self-evolution system is not complete if successful execution cannot be turned into a bounded delivery artifact. W8 needs a delivery stage that prepares branch and pull-request outputs under policy, while still refusing autonomous merge or release.

This issue makes the loop externally reviewable without removing human control.

## Scope
- define a bounded delivery contract for branch and PR preparation
- encode branch name, PR summary, validation evidence, and delivery status as machine-readable outputs
- require explicit supervision and fail-closed behavior when delivery evidence is incomplete
- keep delivery scope limited to preparing artifacts, not merging or releasing them
- add storyline coverage for successful delivery preparation and denied delivery escalation

## Definition of Done
- successful supervised execution can produce a bounded branch and PR preparation summary
- missing or inconsistent delivery evidence stops the loop fail closed
- delivery outputs are stable and auditable across API contract, events, and tests
- runtime coverage proves a delivery-prepared success path and a denied-delivery negative control

## Non-goals
- no automatic merge
- no automatic publish or release
- no generic GitHub automation outside self-evolution delivery preparation
- no bypass of explicit human approval boundaries

## Required Machine-Readable Outputs
- `delivery_summary`
- `branch_name`
- `pr_title`
- `pr_summary`
- `delivery_status`
- `approval_state`
- `reason_code`

## Minimum Validation
- `cargo test -p oris-evokernel --test evolution_lifecycle_regression delivery_summary_ -- --nocapture`
- `cargo test -p oris-runtime --test agent_self_evolution_travel_network --features full-evolution-experimental -- --nocapture`
- `cargo test --workspace`

---

## Issue Draft 5 (`#238`)

### Title

`[EVO26-W8-05][P1] Closed-Loop Acceptance Gate and Audit Consistency`

### Body

## Why
Once intake, proposal, execution, and delivery contracts exist, the track still needs one final gate that decides whether the supervised closed-loop path is internally consistent and auditable. Without this, the W8 chain could regress into stage-local success while overall evidence drifts across APIs, events, and tests.

This issue closes the W8 loop.

## Scope
- define one acceptance and audit gate for the W8 self-evolution loop
- aggregate selection, proposal, execution, approval, and delivery evidence into one machine-readable gate input
- assert reason-code consistency across API surfaces, events, and regression tests
- update the self-evolution acceptance checklist to reflect the shipped W8 boundary once implemented
- keep the gate fail-closed when critical evidence is missing or inconsistent

## Definition of Done
- one machine-readable acceptance gate can evaluate the bounded W8 self-evolution loop
- missing or conflicting evidence fails closed and does not silently pass the gate
- reason-code semantics remain consistent across candidate selection, proposal, execution, and delivery outcomes
- acceptance documentation is updated to the supervised closed-loop boundary after the implementation ships

## Non-goals
- no autonomous release gate that publishes crates
- no expansion into unrelated runtime audit systems
- no weakening of fail-closed semantics for convenience
- no separate parallel reason-code taxonomy for W8-only paths

## Required Machine-Readable Outputs
- `acceptance_gate_summary`
- `audit_consistency_result`
- `approval_evidence`
- `delivery_outcome`
- `reason_code_matrix`
- `fail_closed`

## Minimum Validation
- `cargo test -p oris-evokernel --test evolution_lifecycle_regression acceptance_gate_ -- --nocapture`
- `cargo test -p oris-runtime --test evolution_feature_wiring --features full-evolution-experimental -- --nocapture`
- `cargo test -p oris-runtime --test agent_self_evolution_travel_network --features full-evolution-experimental -- --nocapture`
- `cargo test --workspace`
