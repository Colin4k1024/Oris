# Supervised Devloop Expansion Design

Date: 2026-03-14
Status: Approved for planning
Issue: `#231 [EVO26-W7-04][P1] Supervised Devloop Expansion Under Fail-Closed Bounds`

## 1. Objective

Expand supervised DEVLOOP beyond a single docs file without widening it into a
general code-mutation path. The expanded path must remain bounded, auditable,
and fail-closed, with the same reason-code contract visible in API responses,
events, and regression tests.

## 2. Baseline

Today `run_supervised_devloop(...)` supports exactly one bounded task class:
`DocsSingleFile`.

Current behavior is already solid on core fail-closed controls:
- out-of-scope requests are rejected before execution
- oversize payloads and validation budgets are rejected
- unsafe patch shapes fail closed
- validation failures and timeouts produce a normalized failure contract
- `EvolutionEvent::MutationRejected` records the same reason-code family used by
  the agent-facing failure contract

The gap is not failure taxonomy. The gap is orchestration scope:
- `BoundedTaskClass` only exposes `DocsSingleFile`
- request classification only accepts one file, and only inspects the first file
- the Stream B strategy explicitly calls for supervised devloop expansion
  beyond a single docs file

That means the current implementation is a bounded demo, not yet a bounded
multi-file workflow.

## 3. Decision

Add one new bounded task class for small multi-file docs work and keep every
other constraint fail-closed.

Chosen approach:
- preserve `DocsSingleFile`
- add `DocsMultiFile`
- keep the allowed surface limited to Markdown files under `docs/`
- add a hard file-count ceiling of 3 files per supervised devloop request
- reuse the existing failure taxonomy instead of inventing new reason codes

Rejected alternatives:
- silently relaxing `DocsSingleFile` to mean "one or more docs files": smaller
  diff, but hides the boundary in the public contract and weakens auditability
- broad orchestrated tasks beyond docs: over-scoped for this issue and expands
  policy surface too early

## 4. Architecture

Update the bounded-task classification path in
`crates/oris-agent-contract/src/lib.rs` and `crates/oris-evokernel/src/core.rs`
so supervised devloop distinguishes between:
- `DocsSingleFile`: exactly 1 file, `docs/*.md`
- `DocsMultiFile`: 2 to 3 files, every file under `docs/`, every file ending in
  `.md`

Introduce a shared request-boundary helper in `core.rs` that:
- normalizes path separators
- rejects empty file entries
- requires every declared file to be unique
- requires every declared file to stay inside the docs Markdown boundary
- returns the matching bounded task class when the file set is in policy

Existing byte, line, sandbox-duration, and validation-budget checks remain
unchanged and continue to run after task classification.

## 5. Behavioral Rules

Classification rules:
- `1` docs Markdown file -> `DocsSingleFile`
- `2..=3` docs Markdown files -> `DocsMultiFile`
- `0` files, `>3` files, duplicate files, non-docs paths, or non-Markdown paths
  -> reject as out of bounded scope

Fail-closed mapping stays intentionally narrow:
- out-of-scope request or file-count overflow -> `PolicyDenied`
- diff byte budget or validation budget overflow -> `PolicyDenied`
- unsafe patch shape or boundary-violating patch application -> `UnsafePatch`
- validation failure -> `ValidationFailed`
- timeout -> `Timeout`

Important assumption:
- this issue expands only the bounded task class surface, not the mutation
  failure taxonomy; therefore file-count overflow is still represented as
  `PolicyDenied`

## 6. Data and Audit Consistency

The following surfaces must agree for supervised devloop failures in both
single-file and multi-file paths:
- `SupervisedDevloopOutcome.failure_contract.reason_code`
- `SupervisedDevloopOutcome.failure_contract.recovery_hint`
- `EvolutionEvent::MutationRejected.reason_code`
- `EvolutionEvent::MutationRejected.recovery_hint`

For successful multi-file requests:
- `SupervisedDevloopOutcome.task_class` must be `DocsMultiFile`
- execution feedback continues to come from the existing
  `capture_from_proposal(...)` path

No new event type is needed for this issue.

## 7. Testing Strategy

Add evokernel regression coverage for:
- approved multi-file docs request executes successfully and reports
  `DocsMultiFile`
- multi-file request mixing `docs/*.md` with an out-of-scope path is rejected by
  policy
- multi-file request exceeding the file-count ceiling is rejected by policy
- failure contract reason code and `MutationRejected` event reason code stay
  identical in the new expanded cases

Add runtime facade coverage for:
- `BoundedTaskClass::DocsMultiFile` being exported through
  `oris_runtime::agent_contract`

Required validation for this issue:
- `cargo test -p oris-evokernel --test evolution_lifecycle_regression supervised_devloop_ -- --nocapture`
- `cargo test -p oris-runtime --test evolution_feature_wiring --features full-evolution-experimental -- --nocapture`
- feature-class release floor from the maintainer matrix before publish

## 8. Scope Boundaries

In scope:
- bounded supervised devloop expansion within docs Markdown tasks
- deterministic policy rejection for multi-file requests
- failure-contract and event reason-code consistency
- runtime contract exposure for the new task class

Out of scope:
- `src/` or general code task execution through supervised devloop
- new failure reason-code variants
- autonomous orchestration outside explicit human approval
- new release-gate or economics behavior

## 9. Acceptance Criteria

The design is complete when:
- supervised devloop supports a bounded multi-file docs workflow
- expanded requests remain fail-closed and auditable
- API response, event log, and regression tests agree on the key failure reason
  codes
- the runtime facade exposes the new bounded task class without breaking the
  existing single-file path
