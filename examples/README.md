# Example Projects

This directory contains standalone workspace example projects (separate from `crates/oris-runtime/examples`).

## Projects

- `oris_starter_axum`:
  - Starter service template for integrating Oris runtime into an Axum backend.
  - Includes durable execution endpoints and health checks.
- `oris_worker_tokio`:
  - Standalone worker loop for teams that already host Oris APIs elsewhere.
  - Covers `poll/heartbeat/ack` in a plain Tokio process.
- `oris_operator_cli`:
  - Concrete operator command-line client for `run/list/inspect/resume/replay/cancel`.
  - Good fit for SRE and incident-response workflows.
- `evo_oris_repo`:
  - Experimental EvoKernel scenario suite for the current repository-backed self-evolution slice.
  - Includes capture/replay, supervised devloop gate, bootstrap, spec, network, economics, coordination, and metrics examples.
  - See `examples/evo_oris_repo/README.md` for the full command matrix.
- `vector_store_surrealdb`:
  - Example integration with SurrealDB vector store.

## Which example should you start from?

| Example | Choose it when | Primary runtime shape |
|---|---|---|
| `oris_starter_axum` | You want to embed Oris directly into a Rust service and own the HTTP layer. | App-local service |
| `oris_worker_tokio` | An execution server already exists and this process should only run work. | Standalone worker |
| `oris_operator_cli` | Operators need direct control-plane access from a terminal. | CLI client |
| `evo_oris_repo` | You want to inspect the checked-in EvoKernel flow, not the HTTP runtime path. | Experimental self-evolution |
| `vector_store_surrealdb` | You are validating vector-store integration, not the execution service path. | Storage integration |

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
cargo run -p oris_worker_tokio
cargo run -p oris_operator_cli -- --help
cargo run -p evo_oris_repo
cargo run -p evo_oris_repo --bin supervised_devloop
cargo run -p evo_oris_repo --bin metrics_health
cargo run -p evo_oris_repo --bin evo_vs_non_evo_benchmark
cargo run -p vector_store_surrealdb
```
