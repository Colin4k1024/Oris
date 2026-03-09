# Experience Assets Snapshot

- generated_at: `2026-03-09T14:41:59.638259+00:00`
- scope: `builtin_plus_runtime_store`
- freeze_id_rule: `gene.id`
- asset_count: `6`
- finalized_count: `6`

## Finalized Experiences

| freeze_id | asset_id | task_class | origin | last_promoted_event_seq |
| --- | --- | --- | --- | --- |
| `builtin-experience-ci-fix-v1` | `builtin-experience-ci-fix-v1` | `ci.fix` | `builtin` | `12` |
| `builtin-experience-docs-rewrite-v1` | `builtin-experience-docs-rewrite-v1` | `docs.rewrite` | `builtin` | `8` |
| `a2a-gene-d34fd6ba063f717417834df4596f0bb57f46412953d8e39c6901d051956a86f5` | `a2a-gene-d34fd6ba063f717417834df4596f0bb57f46412953d8e39c6901d051956a86f5` | `compat.class` | `runtime_store` | `4` |
| `builtin-experience-project-workflow-v1` | `builtin-experience-project-workflow-v1` | `project.workflow` | `builtin` | `None` |
| `builtin-experience-service-bid-v1` | `builtin-experience-service-bid-v1` | `service.bid` | `builtin` | `None` |
| `builtin-experience-task-decomposition-v1` | `builtin-experience-task-decomposition-v1` | `task.decomposition` | `builtin` | `None` |

## Asset Details

### a2a-gene-d34fd6ba063f717417834df4596f0bb57f46412953d8e39c6901d051956a86f5

- freeze_id: `a2a-gene-d34fd6ba063f717417834df4596f0bb57f46412953d8e39c6901d051956a86f5`
- state: `Promoted`
- finalized: `True`
- origin: `runtime_store`
- sources: runtime_store
- task_class: `compat.class`
- task_label: `Compat task`
- template_id: `-`
- summary: `compat task completed`
- last_promoted_event_seq: `4`
- last_promoted_at: `2026-03-06T13:40:43.962626+00:00`
- capsule_ref_only: `True`
- signals: compat.class, compat task, compat-task-1
- strategy:
  - `reported_by=compat-agent`
  - `task_class=compat.class`
  - `task_label=Compat task`
  - `source_capsule=compat-capsule-1`
  - `summary=compat task completed`
- validation:
  - `a2a.tasks.report`
- capsules:
  - capsule_id=`compat-capsule-1`, source_type=`strategy_ref`, gene_id=`a2a-gene-d34fd6ba063f717417834df4596f0bb57f46412953d8e39c6901d051956a86f5`, mutation_id=`None`, run_id=`None`, confidence=`None`, state=`None`, outcome_success=`None`

### builtin-experience-ci-fix-v1

- freeze_id: `builtin-experience-ci-fix-v1`
- state: `Promoted`
- finalized: `True`
- origin: `builtin`
- sources: builtin, runtime_store
- task_class: `ci.fix`
- task_label: `CI fix`
- template_id: `builtin-ci-fix-v1`
- summary: `baseline ci stabilization experience`
- last_promoted_event_seq: `12`
- last_promoted_at: `2026-03-06T14:26:34.420548+00:00`
- capsule_ref_only: `False`
- signals: ci.fix, ci, test, failure
- strategy:
  - `asset_origin=builtin`
  - `task_class=ci.fix`
  - `task_label=CI fix`
  - `template_id=builtin-ci-fix-v1`
  - `summary=baseline ci stabilization experience`
- validation:
  - `builtin-template`
  - `origin=builtin`
- capsules:
  - `-`

### builtin-experience-docs-rewrite-v1

- freeze_id: `builtin-experience-docs-rewrite-v1`
- state: `Promoted`
- finalized: `True`
- origin: `builtin`
- sources: builtin, runtime_store
- task_class: `docs.rewrite`
- task_label: `Docs rewrite`
- template_id: `builtin-docs-rewrite-v1`
- summary: `baseline docs rewrite experience`
- last_promoted_event_seq: `8`
- last_promoted_at: `2026-03-06T14:26:34.395519+00:00`
- capsule_ref_only: `False`
- signals: docs.rewrite, docs, rewrite
- strategy:
  - `asset_origin=builtin`
  - `task_class=docs.rewrite`
  - `task_label=Docs rewrite`
  - `template_id=builtin-docs-rewrite-v1`
  - `summary=baseline docs rewrite experience`
- validation:
  - `builtin-template`
  - `origin=builtin`
- capsules:
  - `-`

### builtin-experience-project-workflow-v1

- freeze_id: `builtin-experience-project-workflow-v1`
- state: `Promoted`
- finalized: `True`
- origin: `builtin`
- sources: builtin
- task_class: `project.workflow`
- task_label: `Project workflow`
- template_id: `builtin-project-workflow-v1`
- summary: `baseline project proposal and merge workflow experience`
- last_promoted_event_seq: `None`
- last_promoted_at: `None`
- capsule_ref_only: `False`
- signals: project.workflow, project, workflow, milestone
- strategy:
  - `asset_origin=builtin`
  - `task_class=project.workflow`
  - `task_label=Project workflow`
  - `template_id=builtin-project-workflow-v1`
  - `summary=baseline project proposal and merge workflow experience`
- validation:
  - `builtin-template`
  - `origin=builtin`
- capsules:
  - `-`

### builtin-experience-service-bid-v1

- freeze_id: `builtin-experience-service-bid-v1`
- state: `Promoted`
- finalized: `True`
- origin: `builtin`
- sources: builtin
- task_class: `service.bid`
- task_label: `Service bid`
- template_id: `builtin-service-bid-v1`
- summary: `baseline service bidding and settlement experience`
- last_promoted_event_seq: `None`
- last_promoted_at: `None`
- capsule_ref_only: `False`
- signals: service.bid, service, bid, economics
- strategy:
  - `asset_origin=builtin`
  - `task_class=service.bid`
  - `task_label=Service bid`
  - `template_id=builtin-service-bid-v1`
  - `summary=baseline service bidding and settlement experience`
- validation:
  - `builtin-template`
  - `origin=builtin`
- capsules:
  - `-`

### builtin-experience-task-decomposition-v1

- freeze_id: `builtin-experience-task-decomposition-v1`
- state: `Promoted`
- finalized: `True`
- origin: `builtin`
- sources: builtin
- task_class: `task.decomposition`
- task_label: `Task decomposition`
- template_id: `builtin-task-decomposition-v1`
- summary: `baseline task decomposition and routing experience`
- last_promoted_event_seq: `None`
- last_promoted_at: `None`
- capsule_ref_only: `False`
- signals: task.decomposition, task, decomposition, planning
- strategy:
  - `asset_origin=builtin`
  - `task_class=task.decomposition`
  - `task_label=Task decomposition`
  - `template_id=builtin-task-decomposition-v1`
  - `summary=baseline task decomposition and routing experience`
- validation:
  - `builtin-template`
  - `origin=builtin`
- capsules:
  - `-`
