# Incident Response Runbook

This runbook covers first-response actions for common Oris production incidents.

Use it together with:

- [Production operations guide](production-operations-guide.md)
- [Runtime schema migrations](runtime-schema-migrations.md)
- [PostgreSQL backup and restore runbook](postgres-backup-restore-runbook.md)

## 1. First Five Minutes

When an alert fires:

1. Identify the affected surface:
   - execution API
   - worker loop
   - persistence
   - operator control plane
2. Capture the current blast radius:
   - impacted threads
   - queue depth
   - current error rate
   - worker availability
3. Freeze risky operator actions:
   - avoid manual replay until replay target and side effects are understood
   - avoid schema changes during active incidents
4. Preserve evidence:
   - logs
   - `/metrics` snapshot
   - audit log window for the incident

## 2. High-Severity Triggers

Treat these as immediate operator attention:

- execution API returns repeated `5xx`
- worker poll path stops dispatching while queue depth is non-zero
- recovery latency exceeds alert threshold for more than 10 minutes
- terminal error rate spikes above the expected baseline
- persistence startup health check fails after deploy
- duplicate replay behavior is suspected

## 3. Triage Data to Collect

Collect these before making state-changing interventions:

```bash
curl -s http://127.0.0.1:8080/healthz
curl -s http://127.0.0.1:8080/metrics
curl -s http://127.0.0.1:8080/v1/jobs?limit=20
```

If operator auth is enabled, also collect:

```bash
curl -s http://127.0.0.1:8080/v1/audit/logs?limit=20
curl -s http://127.0.0.1:8080/v1/dlq
```

Key signals:

- `oris_runtime_queue_depth`
- `oris_runtime_lease_conflict_rate`
- `oris_runtime_recovery_latency_ms`
- `oris_runtime_backpressure_total`
- `oris_runtime_terminal_error_rate`

## 4. Incident Playbooks

### API unavailable

Symptoms:

- `/healthz` fails
- `/v1/jobs/*` returns `5xx`

Actions:

1. Check recent deploy history.
2. Check startup logs for backend health-check failure.
3. Verify persistence path and credentials.
4. If caused by a new deploy, roll back the binary.
5. If persistence schema was upgraded, follow rollback guidance from `runtime-schema-migrations.md`.

### Queue depth rising, no dispatch

Symptoms:

- queue depth grows
- worker poll returns `noop` or no workers are active

Actions:

1. Check worker process liveness.
2. Check `backpressure_total` and active lease limits.
3. Check whether leases are stuck or workers are failing heartbeats.
4. Restart only unhealthy workers first.
5. If leases appear stale, confirm lease expiry and requeue behavior before manual intervention.

### Sustained backpressure

Symptoms:

- `oris_runtime_backpressure_total` increases steadily
- queue depth remains non-zero

Actions:

1. Confirm whether the cause is `worker_limit` or `tenant_limit`.
2. If safe, add workers or increase concurrency limits gradually.
3. If tenant-specific, identify the noisy tenant and reduce intake before increasing limits.
4. Do not remove all guardrails just to clear the queue; that hides the actual capacity problem.

### High terminal error rate

Symptoms:

- `oris_runtime_terminal_error_rate` exceeds baseline
- DLQ growth or repeated failed attempts

Actions:

1. Sample recent jobs and DLQ entries.
2. Determine whether failures are code regressions, bad inputs, or downstream dependency failures.
3. Pause operator-triggered replays until the cause is clear.
4. If a release caused the regression, roll back the release.

### Failed deploy after schema change

Symptoms:

- startup fails fast on schema version or backend health

Actions:

1. Stop workers.
2. Confirm the deployed binary version.
3. Check the runtime schema version in persistence.
4. If the binary is older than the schema, deploy the matching or newer binary.
5. If rollback is required, restore persistence from backup and redeploy the previous binary.

### Duplicate replay suspected

Symptoms:

- an operator reports repeated external side effects from replay

Actions:

1. Stop issuing manual replay requests.
2. Capture the affected `thread_id` and replay target.
3. Review audit logs around replay operations.
4. Check whether the replay used the same target (`checkpoint_id` or current-state target).
5. Preserve external side-effect evidence before re-running anything.

## 5. Safe Manual Actions

These actions are generally safe when performed deliberately:

- inspect job state
- list recent jobs
- inspect interrupts
- read audit logs
- read metrics

These actions require care:

- manual replay
- resume with new payloads
- DLQ replay
- increasing concurrency limits during an active incident

## 6. Recovery Validation

Before declaring the incident mitigated:

1. Confirm API success rate recovered.
2. Confirm queue depth is stabilizing or falling.
3. Confirm terminal error rate is back within baseline.
4. Confirm no new unexpected audit anomalies appear.
5. Run one smoke flow:
   - `run`
   - `inspect`
   - worker `poll`
   - worker `ack`

## 7. Post-Incident Checklist

After mitigation:

1. Record the timeline.
2. Save representative metrics and logs.
3. Note any manual operator actions taken.
4. Identify whether docs, alerts, or automation were missing.
5. Open a follow-up issue if the same failure could recur.

## 8. Escalate to Persistence Recovery

Escalate from normal incident handling to persistence recovery when:

- migration state is inconsistent
- startup health cannot validate the backend
- runtime data appears corrupted
- rollback requires restoration from backup

At that point:

1. stop writers
2. take one final snapshot if safe
3. restore from the latest known-good backup
4. redeploy the matching application version
5. re-run smoke validation before restoring traffic
