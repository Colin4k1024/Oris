# Release v0.35.0

## oris-runtime v0.35.0

### Summary

Implements **AUTO-03: Autonomous Mutation Proposal Contracts** (Issue #266).

Adds the typed contract layer for autonomous mutation proposals, allowing the EvoKernel to produce structured `AutonomousMutationProposal` values with deterministic approval-mode routing (auto-approved vs. human-review) based on per-class risk tier.

### New Types (`oris-agent-contract`)

- `AutonomousApprovalMode` — `AutoApproved` | `RequiresHumanReview`
- `AutonomousProposalReasonCode` — `Proposed`, `DeniedPlanNotApproved`, `DeniedNoTargetScope`, `DeniedWeakEvidence`, `DeniedOutOfBounds`, `UnknownFailClosed`
- `AutonomousProposalScope` — `target_paths: Vec<String>`, `scope_rationale: String`, `max_files: u8`
- `AutonomousMutationProposal` — full proposal struct with `proposal_id`, `plan_id`, `dedupe_key`, `scope`, `expected_evidence`, `rollback_conditions`, `approval_mode`, `proposed`, `reason_code`, `summary`, `denial_condition`, `fail_closed`
- `approve_autonomous_mutation_proposal()` — constructor for an approved proposal
- `deny_autonomous_mutation_proposal()` — constructor for a denied proposal

### New Method (`oris-evokernel`)

- `EvoKernel::propose_autonomous_mutation(plan: &AutonomousTaskPlan) -> AutonomousMutationProposal`
  - Returns `AutoApproved` for `AutonomousRiskTier::Low` task classes
  - Returns `RequiresHumanReview` for `Medium`/`High` risk tier task classes
  - Denies with `DeniedPlanNotApproved` if the plan is not approved
  - Denies with `DeniedNoTargetScope` if the task class is unknown
  - All denials set `fail_closed = true`

### Risk-to-Approval Policy

| `BoundedTaskClass`  | `AutonomousRiskTier` | `AutonomousApprovalMode` |
|---------------------|----------------------|--------------------------|
| `LintFix`           | Low                  | `AutoApproved`           |
| `DocsSingleFile`    | Low                  | `AutoApproved`           |
| `DocsMultiFile`     | Medium               | `RequiresHumanReview`    |
| `CargoDepUpgrade`   | Medium               | `RequiresHumanReview`    |

### Tests

- 5 regression tests (`autonomous_proposal_*`) in `evolution_lifecycle_regression.rs`
- 1 wiring gate test `autonomous_mutation_proposal_types_resolve` in `evolution_feature_wiring.rs`

### Closes

- Issue #266 (AUTO-03)
