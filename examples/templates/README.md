# Example Template Matrix

This directory contains reusable `cargo-generate` project skeletons for external Oris users.

## Templates

| Template | Target scenario | Includes |
|---|---|---|
| `axum_service` | Integrate Oris into a Rust HTTP backend | Axum server, health check, runtime API router, SQLite durable execution |
| `worker_only` | Run workers against an existing execution server | Poll/heartbeat/ack loop with backpressure-aware behavior |
| `operator_cli` | Operate jobs from terminal | `run/list/inspect/resume/replay/cancel` command set |

## Scaffold command

Install the standard template tool once:

```bash
cargo install cargo-generate
```

Generate directly from a local checkout:

```bash
cargo generate --path examples/templates/axum_service --name my-oris-service
cargo generate --path examples/templates/worker_only --name my-oris-worker
cargo generate --path examples/templates/operator_cli --name my-oris-ops
```

Generate from GitHub without cloning the repo first:

```bash
cargo generate --git https://github.com/Colin4k1024/Oris.git --subfolder examples/templates/axum_service --name my-oris-service
cargo generate --git https://github.com/Colin4k1024/Oris.git --subfolder examples/templates/worker_only --name my-oris-worker
cargo generate --git https://github.com/Colin4k1024/Oris.git --subfolder examples/templates/operator_cli --name my-oris-ops
```

## Repository-local fallback

If you are contributing inside this repository and do not want to install `cargo-generate`, use the compatibility renderer:

```bash
bash scripts/scaffold_example_template.sh <template> <target-dir>
```

The compatibility script renders the same `{{project-name}}` / `{{crate_name}}` placeholders used by `cargo-generate`.

## Run after scaffold

- `axum_service`: `cargo run`
- `worker_only`: `cargo run`
- `operator_cli`: `cargo run -- --help`
