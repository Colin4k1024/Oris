# Example Projects

This directory contains standalone workspace example projects (separate from `crates/oris-runtime/examples`).

## Projects

- `oris_starter_axum`:
  - Starter service template for integrating Oris runtime into an Axum backend.
  - Includes durable execution endpoints and health checks.
- `vector_store_surrealdb`:
  - Example integration with SurrealDB vector store.

## Run

From repository root:

```bash
cargo run -p oris_starter_axum
cargo run -p vector_store_surrealdb
```
