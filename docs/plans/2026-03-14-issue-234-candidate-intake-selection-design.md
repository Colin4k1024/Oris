# Issue 234 Candidate Intake and Selection Design

Date: 2026-03-14
Issue: `#234 [EVO26-W8-01][P1] Self-Evolution Candidate Intake and Selection Contracts`
Status: Approved

## Context

W8 starts the transition from constrained replay-driven self-evolution toward supervised closed-loop self-evolution.

This first issue should not attempt to build planning, proposal generation, or execution. Its job is narrower:

- accept a GitHub issue-shaped input
- decide whether it is eligible for bounded self-evolution handling
- produce a machine-readable decision
- reject out-of-bounds candidates fail closed with stable reason codes and recovery hints

The user explicitly chose a minimal first version that supports GitHub issue-style inputs only.

## Goal

Add a bounded candidate intake and selection contract for GitHub issue-style self-evolution work items.

## Non-Goals

This issue does not:

- generate mutation proposals
- execute mutations
- create branches or PRs
- fetch issue data from GitHub APIs at runtime
- support generic backlog items beyond the GitHub issue-shaped schema

## Recommended Approach

The recommended approach is to add a dedicated selection stage alongside the existing supervised devloop contracts, not inside them.

Why:

- selection and execution remain cleanly separated for W8-02 and W8-03
- the contract can stay small and machine-readable
- fail-closed reason-code behavior can be tested independently of mutation execution

## Contract Shape

### Input

Add a GitHub issue-shaped intake request in `oris-agent-contract`.

Recommended fields:

- `issue_number: u64`
- `title: String`
- `body: String`
- `labels: Vec<String>`
- `state: String`
- `candidate_hint_paths: Vec<String>`

Rationale:

- issue number gives stable identity
- title and body give bounded classification context
- labels and state give deterministic policy signals
- candidate hint paths allow the caller to declare intended file scope without introducing proposal generation yet

### Output

Add a machine-readable selection decision contract.

Recommended fields:

- `issue_number: u64`
- `selected: bool`
- `candidate_class: Option<BoundedTaskClass>`
- `summary: String`
- `reason_code: Option<SelfEvolutionSelectionReasonCode>`
- `failure_reason: Option<String>`
- `recovery_hint: Option<String>`
- `fail_closed: bool`

The output should encode both accept and reject decisions without needing a second failure wrapper type.

## Candidate Boundary Rules

First-version bounded policy:

- `state` must be `OPEN`
- labels must include `area/evolution`
- labels must include `type/feature`
- labels must not include `duplicate`, `invalid`, or `wontfix`
- `candidate_hint_paths` must classify into an existing bounded docs task class using the current docs-only boundary

This means the selector remains aligned with the current supervised devloop limit instead of inventing a broader execution scope before W8-02/W8-03.

## Reason Codes

Add a dedicated selection reason-code enum instead of overloading mutation-needed codes.

Recommended variants:

- `Accepted`
- `IssueClosed`
- `MissingEvolutionLabel`
- `MissingFeatureLabel`
- `ExcludedByLabel`
- `UnsupportedCandidateScope`
- `UnknownFailClosed`

Notes:

- `Accepted` keeps positive-path audit output machine-readable
- reject cases should map to stable default failure text and recovery hints
- unknown states should fail closed instead of defaulting to acceptance

## Runtime Placement

### `oris-agent-contract`

Add:

- `SelfEvolutionCandidateIntakeRequest`
- `SelfEvolutionSelectionReasonCode`
- `SelfEvolutionSelectionDecision`
- helper to normalize a reject decision with default text/hints

### `oris-evokernel`

Add:

- `select_self_evolution_candidate(&self, request: &SelfEvolutionCandidateIntakeRequest) -> SelfEvolutionSelectionDecision`

Selection should:

- normalize labels and state case-insensitively
- reuse existing docs-file normalization logic where possible
- classify accepted candidates to an existing `BoundedTaskClass`
- return a stable fail-closed reject decision when the candidate is outside policy

### `oris-runtime`

Expose the new contract types and kernel method through the existing evolution facade so runtime feature wiring can lock the public surface.

## Error Handling

This issue should not return ad hoc strings alone.

Rules:

- all rejects are explicit and fail closed
- all reject paths must include `reason_code`, `failure_reason`, `recovery_hint`, and `fail_closed = true`
- accepted results should include `selected = true`, a `candidate_class`, and `fail_closed = false`
- malformed or unknown input states should map to `UnknownFailClosed`

## Testing Strategy

### EvoKernel regression

Add tests for:

- accepts an open evolution feature issue bounded to one docs markdown file
- rejects a closed issue with `IssueClosed`
- rejects an issue missing `area/evolution`
- rejects an issue missing `type/feature`
- rejects an issue with excluded labels such as `duplicate`
- rejects an issue whose candidate paths are outside the current bounded docs scope

### Runtime wiring

Add coverage that the runtime facade exports:

- `SelfEvolutionCandidateIntakeRequest`
- `SelfEvolutionSelectionReasonCode`
- `SelfEvolutionSelectionDecision`
- `EvoKernel::select_self_evolution_candidate`

## Validation Plan

Targeted validation floor for the issue:

- `cargo test -p oris-evokernel --test evolution_lifecycle_regression candidate_intake_ -- --nocapture`
- `cargo test -p oris-runtime --test evolution_feature_wiring --features full-evolution-experimental -- --nocapture`

Required feature-level validation before release:

- `cargo fmt --all -- --check`
- `cargo test --workspace`
- `cargo build --verbose --all --release --all-features`
- `cargo test --release --all-features`
