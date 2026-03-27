# v1.0 Operator Quickstart and Diagnostics Guide

**Version:** v1.0
**Date:** 2026-03-27

## Table of Contents

1. [Quickstart](#quickstart)
2. [Diagnostics](#diagnostics)
3. [Operational Runbooks](#operational-runbooks)
4. [Reference Documentation](#reference-documentation)

---

## Quickstart

### Starting the Execution Server

```bash
# Build with all features
cargo build --all --release --all-features

# Run the execution server
cargo run -p oris-runtime --example execution_server --features "sqlite-persistence,execution-server"
```

The server starts on `http://127.0.0.1:8080` by default (configurable via `ORIS_SERVER_ADDR`).

### Submitting a Test Job

```bash
# Using curl to submit a job
curl -X POST http://127.0.0.1:8080/jobs \
  -H "Content-Type: application/json" \
  -d '{
    "graph_name": "test_graph",
    "input": {"task": "example task"},
    "config": {
      "checkpoint_interval": 10,
      "enable_eviction": true
    }
  }'
```

### Monitoring Job Status

```bash
# Check job status
curl http://127.0.0.1:8080/jobs/<job_id>

# Stream job events
curl -N http://127.0.0.1:8080/jobs/<job_id>/stream
```

### Running Examples

```bash
# Run the canonical evolution example
cargo run -p evo_oris_repo

# Run evolution bins
cargo run -p evo_oris_repo --bin intake_webhook_demo
cargo run -p evo_oris_repo --bin confidence_lifecycle_demo
cargo run -p evo_oris_repo --bin network_exchange

# Run the Axum starter
cargo run -p oris_starter_axum

# Run the operator CLI
cargo run -p oris_operator_cli
```

---

## Diagnostics

### Checking Kernel Logs

The kernel uses structured logging via the `tracing` crate. Configure log level via environment:

```bash
# Set log level
export RUST_LOG=debug

# Run with debug output
cargo run -p oris-runtime --example execution_server 2>&1 | grep -E "(kernel|event|replay)"
```

### Inspecting Replay State

Use the operator CLI to inspect kernel state:

```bash
# Check kernel status
cargo run -p oris_operator_cli -- status

# List recent runs
cargo run -p oris_operator_cli -- runs list --limit 10

# Inspect a specific run
cargo run -p oris_operator_cli -- runs inspect <run_id>

# Check replay cursor position
cargo run -p oris_operator_cli -- runs replay-cursor <run_id>
```

### Troubleshooting Failed Jobs

1. **Check job terminal state:**
   ```bash
   curl http://127.0.0.1:8080/jobs/<job_id>
   ```

2. **Inspect failure reason:**
   ```bash
   cargo run -p oris_operator_cli -- runs inspect <run_id> --show-events
   ```

3. **Check for interrupt/resume issues:**
   ```bash
   cargo run -p oris_operator_cli -- runs interrupt-status <run_id>
   ```

4. **View snapshot history:**
   ```bash
   cargo run -p oris_operator_cli -- runs snapshots <run_id>
   ```

### Common Diagnostic Commands

| Command | Purpose |
|---------|---------|
| `oris_operator_cli status` | Overall system status |
| `oris_operator_cli runs list` | List recent runs |
| `oris_operator_cli runs inspect <id>` | Inspect run details |
| `oris_operator_cli runs events <id>` | List run events |
| `oris_operator_cli runs replay-cursor <id>` | Show replay position |
| `oris_operator_cli runs interrupt-status <id>` | Check interrupt state |
| `oris_operator_cli leases list` | List active leases |
| `oris_operator_cli health` | Health check |

### Event Log Inspection

Kernel events are stored in SQLite:

```bash
# Direct SQLite inspection
sqlite3 oris.db "SELECT run_id, seq, event_json FROM kernel_events WHERE run_id = '<run_id>' ORDER BY seq"
```

### Snapshot Inspection

```bash
# List snapshots for a run
sqlite3 oris.db "SELECT run_id, at_seq, state_json FROM kernel_snapshots WHERE run_id = '<run_id>' ORDER BY at_seq"
```

---

## Operational Runbooks

### Incident Response

Reference: [incident-response-runbook.md](incident-response-runbook.md)

**Steps:**
1. Identify the affected run/job
2. Check kernel event log for errors
3. Determine if replay is possible
4. Execute recovery procedure

### Schema Migrations

Reference: [runtime-schema-migrations.md](runtime-schema-migrations.md)

**Migrations are automatic on startup.** To manually trigger:
```bash
cargo run -p oris_operator_cli -- migrate --force
```

### Backup and Restore

Reference: [postgres-backup-restore-runbook.md](postgres-backup-restore-runbook.md)

**SQLite:**
```bash
# Backup
cp oris.db oris.db.backup-$(date +%Y%m%d-%H%M%S)

# Restore
cp oris.db.backup-<timestamp> oris.db
```

**PostgreSQL:**
```bash
# Backup
pg_dump oris > oris-backup-$(date +%Y%m%d-%H%M%S).sql

# Restore
psql oris < oris-backup-<timestamp>.sql
```

### Crash Recovery

Reference: [v100-runtime-hardening-baseline.md](v100-runtime-hardening-baseline.md)

The kernel automatically recovers from crashes via:
1. Event log replay (source of truth)
2. Snapshot optimization (for fast replay)
3. Determinism guard (traps nondeterminism)

### Scheduler Backpressure

Reference: [scheduler-stress-baseline.md](scheduler-stress-baseline.md)

When backpressure is triggered:
1. Check per-tenant throttle status
2. Verify per-worker queue depth
3. Review circuit breaker state

```bash
cargo run -p oris_operator_cli -- scheduler stats
```

---

## Reference Documentation

| Document | Purpose |
|----------|---------|
| [ARCHITECTURE.md](ARCHITECTURE.md) | System architecture overview |
| [kernel-api.md](kernel-api.md) | Kernel API reference |
| [evolution.md](evolution.md) | Evolution system overview |
| [incident-response-runbook.md](incident-response-runbook.md) | Incident response procedures |
| [runtime-schema-migrations.md](runtime-schema-migrations.md) | Database schema migrations |
| [postgres-backup-restore-runbook.md](postgres-backup-restore-runbook.md) | Backup and restore |
| [replay-lifecycle-invariants.md](replay-lifecycle-invariants.md) | Replay invariants |
| [interrupt-resume-invariants.md](interrupt-resume-invariants.md) | Interrupt/resume invariants |
| [v100-release-proof-artifacts.md](v100-release-proof-artifacts.md) | v1.0 proof artifacts |

---

## Environment Variables

| Variable | Default | Purpose |
|----------|---------|---------|
| `ORIS_SERVER_ADDR` | `127.0.0.1:8080` | Server address |
| `ORIS_SQLITE_DB` | `oris.db` | SQLite database path |
| `ORIS_RUNTIME_BACKEND` | `sqlite` | Runtime backend (`sqlite` or `postgres`) |
| `RUST_LOG` | `info` | Log level |
| `OPENAI_API_KEY` | - | OpenAI API key (if using LLM features) |
| `ANTHROPIC_API_KEY` | - | Anthropic API key |
| `OLLAMA_HOST` | `http://localhost:11434` | Ollama host |

---

## Feature Flags

Key feature flags for v1.0 operation:

| Flag | Purpose |
|------|---------|
| `sqlite-persistence` | SQLite checkpointing |
| `execution-server` | HTTP API server |
| `full-evolution-experimental` | All evolution features |
| `governor-experimental` | Governor policies |
| `evolution-network-experimental` | Evolution network |
