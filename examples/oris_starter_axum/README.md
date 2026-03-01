# Oris Starter (Axum)

This is a starter service project that shows how to use Oris as part of a Rust web backend.

## What this example demonstrates

- Running Oris runtime inside an Axum HTTP service.
- SQLite-backed durable execution and idempotency.
- Operator-facing endpoints for run/list/inspect/resume/replay/cancel.
- Basic health endpoint and tracing setup.

## Run

From repository root:

```bash
cargo run -p oris_starter_axum
```

Optional environment variables:

- `ORIS_SERVER_ADDR` (default: `127.0.0.1:8080`)
- `ORIS_SQLITE_DB` (default: `oris_starter.db`)
- `ORIS_RUNTIME_BACKEND` (`sqlite` default; `postgres` requires `kernel-postgres` feature)
- `ORIS_POSTGRES_DSN` or `ORIS_RUNTIME_DSN` (required when `ORIS_RUNTIME_BACKEND=postgres`)
- `ORIS_POSTGRES_SCHEMA` (default: `public`)
- `ORIS_POSTGRES_REQUIRE_SCHEMA` (default: `true`; fail fast when schema is missing)
- `ORIS_API_AUTH_BEARER_TOKEN` (optional; when set, requests must send `Authorization: Bearer <token>`)
- `ORIS_API_AUTH_API_KEY` (optional; when set, requests may send `x-api-key: <key>`)
- `ORIS_API_AUTH_API_KEY_ID` (optional; when set with `ORIS_API_AUTH_API_KEY`, requests should send `x-api-key-id` + `x-api-key`)

Startup now performs backend health checks and exits non-zero on invalid backend config (for example invalid DSN or missing required schema).

When `ORIS_API_AUTH_API_KEY_ID` + `ORIS_API_AUTH_API_KEY` are provided, starter will persist the keyed API credential into SQLite table `runtime_api_keys` on startup.
Persisted keyed credentials default to `operator` role (jobs/interrupts, `/v1/dlq*`, `GET /v1/audit/logs`, and `GET /v1/attempts/:attempt_id/retries` allowed; worker APIs denied).

## Quick API smoke test

Create a run:

```bash
curl -s -X POST http://127.0.0.1:8080/v1/jobs \
  -H 'content-type: application/json' \
  -H 'Authorization: Bearer <token-if-enabled>' \
  -H 'x-api-key-id: <key-id-if-enabled>' \
  -H 'x-api-key: <key-if-enabled>' \
  -d '{"thread_id":"starter-1","input":"hello from starter","idempotency_key":"starter-key-1"}'
```

Inspect run:

```bash
curl -s http://127.0.0.1:8080/v1/jobs/starter-1
```

List runs:

```bash
curl -s http://127.0.0.1:8080/v1/jobs
```

## Where to go next

- Add your own graph nodes and tool calls.
- Integrate JWT/API-key verification backend (replace static env secrets).
- Emit traces/metrics to your observability backend.
- Replace SQLite with your production persistence strategy as needed.

See also:

- `docs/rust-ecosystem-integration.md`
- `crates/oris-runtime/examples/`
