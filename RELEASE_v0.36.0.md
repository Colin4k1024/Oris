# Release v0.36.0

## oris-runtime v0.36.0

### Summary

Implements **AUTO-04: Semantic Task-Class Generalization Beyond Normalized Signals** (Issue #267).

Adds a typed semantic equivalence layer that allows replay selection to generalize beyond exact normalized signal matching to broader task families. False-positive prevention is maintained: only explicitly approved families are replayed; unrelated tasks do not replay.

### New Types (`oris-agent-contract`)

- `TaskEquivalenceClass` — semantic family groupings: `DocumentationEdit`, `StaticAnalysisFix`, `DependencyManifestUpdate`, `Unclassified`
- `EquivalenceExplanation` — structured audit record: `task_equivalence_class`, `rationale`, `matching_features`, `replay_match_confidence`
- `SemanticReplayReasonCode` — `EquivalenceMatchApproved`, `LowConfidenceDenied`, `NoEquivalenceClassMatch`, `EquivalenceClassNotAllowed`, `UnknownFailClosed`
- `SemanticReplayDecision` — `evaluation_id`, `task_id`, `replay_decision`, `equivalence_explanation`, `reason_code`, `summary`, `fail_closed`
- `approve_semantic_replay()` — constructor for an approved decision
- `deny_semantic_replay()` — constructor for a denied decision (fail_closed=true)

### New Method (`oris-evokernel`)

- `EvoKernel::evaluate_semantic_replay(task_id, task_class: &BoundedTaskClass) -> SemanticReplayDecision`
  - Low-risk classes (`LintFix`, `DocsSingleFile`) → `EquivalenceMatchApproved`, `fail_closed=false`
  - Medium-risk classes (`DocsMultiFile`, `CargoDepUpgrade`) → `EquivalenceClassNotAllowed`, `fail_closed=true`
  - Returns full `EquivalenceExplanation` with matching_features and replay_match_confidence for approved decisions

### Semantic Equivalence Policy

| `BoundedTaskClass`  | `TaskEquivalenceClass`         | Auto-Replay | Confidence |
|---------------------|-------------------------------|-------------|------------|
| `LintFix`           | `StaticAnalysisFix`           | Yes         | 95         |
| `DocsSingleFile`    | `DocumentationEdit`           | Yes         | 90         |
| `DocsMultiFile`     | `DocumentationEdit`           | No (human review) | 75  |
| `CargoDepUpgrade`   | `DependencyManifestUpdate`    | No (human review) | 72  |

### Tests

- 5 regression tests (`semantic_replay_*`) in `evolution_lifecycle_regression.rs`
- 1 wiring gate test `semantic_replay_decision_types_resolve` in `evolution_feature_wiring.rs`

### Closes

- Issue #267 (AUTO-04)
