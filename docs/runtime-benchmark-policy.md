# Runtime Benchmark Policy

Oris keeps runtime performance measurement separate from correctness tests.

## Harness

The dedicated benchmark harness lives outside the test suite:

- Runner script: `scripts/run_runtime_benchmark_suite.sh`
- Example entrypoint: `crates/oris-runtime/examples/runtime_benchmark_suite.rs`
- Current checked-in baseline: [runtime-benchmark-baseline.json](/Users/jiafan/Desktop/poc/Oris/docs/runtime-benchmark-baseline.json)

It measures the current hot paths with a repeatable fixed-sample loop:

- scheduler dispatch claim
- lease heartbeat
- `POST /v1/jobs/run`
- `GET /v1/jobs/:thread_id`
- `GET /v1/jobs`
- `POST /v1/jobs/:thread_id/replay`

The output format is stable JSON so results can be diffed or artifacted
consistently across runs.

## Commands

Generate a local report without touching the checked-in baseline:

```bash
./scripts/run_runtime_benchmark_suite.sh target/runtime-benchmark-latest.json 5
```

Refresh the checked-in baseline intentionally:

```bash
./scripts/run_runtime_benchmark_suite.sh docs/runtime-benchmark-baseline.json 5
```

## CI policy

CI runs the benchmark harness in its own job and uploads the JSON report as an
artifact. This job validates that:

- the benchmark harness still runs
- the report format remains stable

CI does **not** fail merges on an exact performance threshold. Shared runners are
too noisy for strict automatic gating.

## Regression review policy

Performance changes are reviewed manually against the checked-in baseline:

1. For changes touching runtime scheduling, lease handling, replay, or execution
   server handlers, inspect the benchmark artifact from CI.
2. Compare the artifact to `docs/runtime-benchmark-baseline.json`.
3. Treat any sustained slowdown above roughly 20% in `avg_ms` or
   `throughput_per_sec` on a hot path as review-blocking until explained.
4. If a slowdown is intentional, update the checked-in baseline in the same PR
   and document the reason.

The baseline is a regression reference, not a production SLA.
