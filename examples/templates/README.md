# Example Template Matrix

This directory contains reusable project skeletons for external Oris users.

## Templates

| Template | Target scenario | Includes |
|---|---|---|
| `axum_service` | Integrate Oris into a Rust HTTP backend | Axum server, health check, runtime API router, SQLite durable execution |
| `worker_only` | Run workers against an existing execution server | Poll/heartbeat/ack loop with backpressure-aware behavior |
| `operator_cli` | Operate jobs from terminal | `run/list/inspect/resume/replay/cancel` command set |

## Scaffold command

From repository root:

```bash
bash scripts/scaffold_example_template.sh <template> <target-dir>
```

Examples:

```bash
bash scripts/scaffold_example_template.sh axum_service /tmp/my-oris-service
bash scripts/scaffold_example_template.sh worker_only /tmp/my-oris-worker
bash scripts/scaffold_example_template.sh operator_cli /tmp/my-oris-ops
```

The script replaces `__CRATE_NAME__` automatically based on `<target-dir>` basename.
