# worker_only template

Use this template when execution APIs are hosted elsewhere and this process is only a worker.

## What you get

- Poll loop against `/v1/workers/poll`
- Lease heartbeat updates
- Attempt acknowledgement (`completed` by default in template)
- Backpressure/noop handling

## Suggested next edits

1. Replace `execute_attempt` placeholder with real job execution.
2. Add `report-step` events for each actionable stage.
3. Add retry and failure classification strategy before marking `failed`.
