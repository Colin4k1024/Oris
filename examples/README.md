# Example Projects

This directory contains standalone workspace example projects (separate from `crates/oris-runtime/examples`).

## Projects

- `oris_starter_axum`:
  - Starter service template for integrating Oris runtime into an Axum backend.
  - Includes durable execution endpoints and health checks.
- `vector_store_surrealdb`:
  - Example integration with SurrealDB vector store.

## Template matrix

- `templates/axum_service`:
  - Blueprint for app-local Axum service + Oris runtime API.
- `templates/worker_only`:
  - Blueprint for standalone worker loop (`poll/heartbeat/ack`).
- `templates/operator_cli`:
  - Blueprint for operator command-line client (`run/list/inspect/resume/replay/cancel`).

Scaffold a new project from template with `cargo-generate`:

```bash
cargo install cargo-generate
cargo generate --path examples/templates/axum_service --name my-oris-service
cargo generate --path examples/templates/worker_only --name my-oris-worker
cargo generate --path examples/templates/operator_cli --name my-oris-ops
```

If you are working from a local checkout and want a no-install fallback, use:

```bash
bash scripts/scaffold_example_template.sh <template> <target-dir>
```

## Run

From repository root:

```bash
cargo run -p oris_starter_axum
cargo run -p vector_store_surrealdb
```
