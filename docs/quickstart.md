# Oris Quickstart

Get Oris running locally in under 5 minutes.

## Prerequisites

- Rust 1.80+ (`rustup toolchain install stable`)
- Git (for cloning the repo)
- An OpenAI API key (for LLM-backed mutation evaluation, optional for basic pipeline)

## 1. Clone and Build

```bash
git clone https://github.com/oris-project/oris.git
cd oris
cargo build --release
```

## 2. Run the Canonical Evolution Scenario

The simplest way to see Oris in action:

```bash
cargo run -p evo_oris_repo
```

This runs a full Detect -> Select -> Mutate -> Execute -> Validate cycle using in-memory fixtures.

## 3. Run the First-Run Script (with Artifacts)

For observable JSON output:

```bash
bash scripts/evo_first_run.sh
```

Produces:
- `target/evo_first_run/summary.json` — pass/fail status, timing, error codes
- `target/evo_first_run/run.log` — full execution log

## 4. Using Oris as a Library

Add to your `Cargo.toml`:

```toml
[dependencies]
oris-runtime = "0.61"
```

### Feature Flags

Enable capabilities incrementally:

| Feature | What it enables |
|---------|----------------|
| `sqlite-persistence` | Durable checkpointing via SQLite |
| `execution-server` | HTTP API server (axum-based) |
| `evokernel-facade` | Self-evolution kernel re-exports |
| `evolution-experimental` | Full evolution pipeline (detect/select/mutate/validate) |
| `full-evolution-experimental` | All experimental evolution features combined |

Example with evolution + persistence:

```toml
[dependencies]
oris-runtime = { version = "0.61", features = ["sqlite-persistence", "evolution-experimental"] }
```

## 5. Running the Execution Server

Start the HTTP execution server for submitting and monitoring jobs:

```bash
cargo run -p oris-runtime --example execution_server \
  --features "sqlite-persistence,execution-server"
```

Server runs on `http://127.0.0.1:8080` (override with `ORIS_SERVER_ADDR`).

### Submit a job

```bash
curl -X POST http://127.0.0.1:8080/jobs \
  -H "Content-Type: application/json" \
  -d '{"graph_name": "test_graph", "input": {"task": "example"}}'
```

### Check status

```bash
curl http://127.0.0.1:8080/jobs/<job_id>
```

## 6. Running the Intake Webhook

Process CI failures automatically via HTTP webhook:

```bash
cargo run -p evo_oris_repo --bin intake_webhook_demo
```

Posts test failure events to the intake pipeline. Supports cargo test output, clippy warnings, and GitHub Actions annotations.

## 7. Observability

### OTel Tracing

The evolution pipeline emits OpenTelemetry-compatible spans via the `tracing` crate:

- `evolution.detect`
- `evolution.select`
- `evolution.mutate`
- `evolution.execute`
- `evolution.validate`

Configure a tracing subscriber with your preferred exporter (Jaeger, OTLP, etc.) to collect these spans.

### Prometheus Metrics

Enable the `prometheus` feature on `oris-evokernel`:

```toml
oris-evokernel = { version = "0.14", features = ["prometheus"] }
```

Exposes 5 core metrics:

| Metric | Type | Description |
|--------|------|-------------|
| `oris_evolution_cycles_total` | Counter | Completed pipeline cycles |
| `oris_confidence_distribution` | Histogram | Gene confidence score distribution |
| `oris_intake_queue_depth` | Gauge | Pending intake events |
| `oris_acceptance_rate` | Gauge | Accepted vs total proposals (0.0-1.0) |
| `oris_replay_hit_rate` | Gauge | Replay cache hit ratio (0.0-1.0) |

Wire into an axum handler:

```rust
use oris_evokernel::metrics::EvolutionMetrics;

let metrics = EvolutionMetrics::new();
let app = axum::Router::new()
    .route("/metrics", axum::routing::get({
        let m = metrics.clone();
        move || async move { m.encode() }
    }));
```

## 8. Running Tests

```bash
# All tests (requires all features)
cargo test --release --all-features

# Specific crate
cargo test -p oris-evolution

# Specific test
cargo test -p oris-intake ci_parser
```

## 9. Example Projects

| Example | Command | Purpose |
|---------|---------|---------|
| `evo_oris_repo` | `cargo run -p evo_oris_repo` | Canonical evolution scenario |
| `oris_starter_axum` | `cargo run -p oris_starter_axum` | Axum web server integration |
| `oris_worker_tokio` | `cargo run -p oris_worker_tokio` | Background worker pattern |
| `oris_operator_cli` | `cargo run -p oris_operator_cli` | CLI operations tool |

## Next Steps

- [Architecture overview](ARCHITECTURE.md)
- [Kernel API reference](kernel-api.md)
- [Evolution boundary design](evolution-boundary.md)
- [Production operations guide](production-operations-guide.md)
- [Plugin authoring](plugin-authoring.md)
