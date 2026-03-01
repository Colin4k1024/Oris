# Production Operations Guide

This guide gives operators an end-to-end path from a fresh Oris deployment to a production-ready operating baseline.

## Scope

This guide covers:

- fresh deployment
- runtime configuration
- upgrade and migration flow
- production readiness checks
- SLO baseline
- links to backup, restore, and incident runbooks

Existing detailed references:

- [Runtime schema migrations](runtime-schema-migrations.md)
- [PostgreSQL backup and restore runbook](postgres-backup-restore-runbook.md)
- [Incident response runbook](incident-response-runbook.md)

## 1. Deployment Model

Recommended production topology:

1. One execution API process exposing `/v1/jobs*`, `/v1/interrupts*`, `/v1/workers*`, `/metrics`.
2. One or more worker processes polling the execution API.
3. Durable persistence enabled from day one:
   - SQLite for single-node or low-scale private deployments.
   - PostgreSQL when schema-level operational controls, backups, and external DB ops are required.
4. A reverse proxy or ingress in front of operator-facing APIs.

Minimum process separation:

- business/API ingress
- execution API
- worker loop
- database

## 2. Fresh Deploy Checklist

Before first boot, prepare:

1. A Rust binary built with the required features:
   - `sqlite-persistence` for SQLite-backed runtime.
   - `kernel-postgres` if you need PostgreSQL runtime repository health checks.
   - `execution-server` for the HTTP execution API.
2. A writable persistence target:
   - `ORIS_SQLITE_DB=/path/to/oris_runtime.db`
   - or `ORIS_RUNTIME_BACKEND=postgres` with `ORIS_POSTGRES_DSN` and `ORIS_POSTGRES_SCHEMA`
3. Auth secrets for operator APIs:
   - `ORIS_API_AUTH_BEARER_TOKEN`
   - or `ORIS_API_AUTH_API_KEY`
4. Bind address:
   - `ORIS_SERVER_ADDR=127.0.0.1:8080` (or your production interface)
5. Log and trace routing:
   - capture stdout/stderr
   - ship structured logs with request IDs

## 3. First Boot Procedure

Use this sequence for a new environment:

1. Start the execution server.
2. Confirm startup health passed (the server now fails fast on invalid backend config).
3. Verify `/healthz` and `/metrics`.
4. Run one `POST /v1/jobs/run` smoke test.
5. Run one worker poll cycle and one ack cycle.
6. Confirm audit and metrics are emitted.

Example local boot:

```bash
ORIS_SQLITE_DB=oris_runtime.db \
cargo run -p oris-runtime --example execution_server --features "sqlite-persistence,execution-server"
```

Smoke checks:

```bash
curl -s http://127.0.0.1:8080/healthz
curl -s http://127.0.0.1:8080/metrics
curl -s -X POST http://127.0.0.1:8080/v1/jobs/run \
  -H 'content-type: application/json' \
  -d '{"thread_id":"ops-smoke-1","input":"hello","idempotency_key":"ops-smoke-key-1"}'
curl -s http://127.0.0.1:8080/v1/jobs/ops-smoke-1
```

Success criteria:

- server accepts requests
- persistence initializes successfully
- one run can be created and inspected
- metrics endpoint is scrapeable
- logs include `request_id`

## 4. Upgrade and Migration Workflow

Every upgrade should follow the same sequence:

1. Confirm the target binary version and enabled features.
2. Take a backup before introducing any binary that may apply schema migrations.
3. Quiesce writes:
   - stop new operator traffic
   - drain or stop workers
4. Deploy the new binary to the execution API.
5. Allow the runtime repository to apply forward-only migrations.
6. Run smoke tests:
   - `run`
   - `inspect`
   - worker `poll`
   - worker `ack`
7. Re-enable worker traffic.
8. Re-enable operator traffic.

If migrations are involved:

- Use [Runtime schema migrations](runtime-schema-migrations.md) for exact rollback rules.
- Use [PostgreSQL backup and restore runbook](postgres-backup-restore-runbook.md) before PostgreSQL upgrades.

Rollback rule:

- schema rollback is operational, not in-place
- restore from backup, redeploy the previous binary, then re-enable traffic

## 5. Configuration Baseline

Recommended production environment variables:

- `ORIS_SERVER_ADDR`
- `ORIS_RUNTIME_BACKEND`
- `ORIS_SQLITE_DB` or `ORIS_POSTGRES_DSN`
- `ORIS_POSTGRES_SCHEMA`
- `ORIS_POSTGRES_REQUIRE_SCHEMA=true`
- `ORIS_API_AUTH_BEARER_TOKEN` or `ORIS_API_AUTH_API_KEY`
- `RUST_LOG=info,oris_runtime=info`

Worker-side controls:

- `ORIS_WORKER_ID`
- bounded worker concurrency via `max_active_leases`
- tenant guardrails via `tenant_max_active_leases`

Operational policy:

- never run production operator APIs without auth
- keep `thread_id` stable and tied to a business entity
- always use `idempotency_key` for externally triggered runs

## 6. Production Readiness Gate

Do not mark an environment production-ready until all of these are true:

1. Durability:
   - crash recovery has been validated
   - persistence is backed up on a scheduled basis
2. Replay safety:
   - replay guard is enabled under `sqlite-persistence`
   - duplicate replay requests do not repeat side effects
3. Scheduler safety:
   - failover/lease-expiry behavior has been validated
   - worker and tenant backpressure limits are configured
4. Observability:
   - Prometheus scrapes `/metrics`
   - alerts are loaded from `docs/observability/prometheus-alert-rules.yml`
   - Grafana dashboard is provisioned from `docs/observability/runtime-dashboard.json`
5. Security:
   - operator credentials are rotated and stored outside source control
   - audit logs are retained in persistence
6. Documentation:
   - on-call operators have access to this guide and the incident runbook

## 7. SLO Baseline

These are default operator-facing starting targets, not hard product guarantees.

Suggested baseline:

- API availability: 99.9% monthly for `/v1/jobs*` and `/v1/workers*`
- successful dispatch latency: p95 under 250 ms in steady state
- recovery latency after forced lease expiry: p95 under 1000 ms
- sustained backpressure: no more than 10 continuous minutes without operator action
- replay safety: zero duplicate side-effect executions for the same replay target

Suggested alert thresholds:

- terminal error rate above 5%
- p95 recovery latency above 1000 ms
- sustained backpressure over 10 minutes with non-zero queue depth
- repeated lease conflict growth beyond normal baseline

Use the shipped observability assets as the default implementation:

- `docs/observability/runtime-dashboard.json`
- `docs/observability/prometheus-alert-rules.yml`
- `docs/observability/sample-runtime-workload.prom`

## 8. Daily and Weekly Operations

Daily:

1. Check alert status.
2. Confirm `/metrics` scrape freshness.
3. Review queue depth, recovery latency, and error rate.
4. Review audit logs for sensitive control-plane actions.

Weekly:

1. Rehearse backup/restore in a non-production environment.
2. Review worker concurrency and backpressure settings.
3. Review idempotency and replay-related incidents.
4. Confirm upgrade rollback artifacts are current.

## 9. Escalation Paths

When production behavior deviates from baseline:

1. Use [Incident response runbook](incident-response-runbook.md) for immediate triage.
2. If persistence integrity is in doubt, stop writers first.
3. If schema mismatch is suspected, validate migration version before restarting workers.
4. If repeated replay or timeout anomalies appear, preserve logs and audit entries before any manual replays.

## 10. Minimum Operator Handoff

The handoff package for an on-call operator should include:

- service endpoints
- auth bootstrap method
- current persistence backend
- last successful backup time
- current deployed version
- alert routing destination
- this guide
- migration and incident runbooks
