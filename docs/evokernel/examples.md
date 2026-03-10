# Evo Example Programs

Date: March 5, 2026

This document describes the Evolution example programs under `examples/evo_oris_repo`.

All commands run from repository root.

## First run (30 minutes)

Use the canonical onboarding entry first:

```bash
bash scripts/evo_first_run.sh
```

Expected observable artifacts:

- `target/evo_first_run/summary.json`
- `target/evo_first_run/run.log`

After this passes, continue with the advanced scenario list below.

## 1. Canonical capture/replay flow

Command:

```bash
cargo run -p evo_oris_repo
```

Shows:

- supervised docs task execution through `run_supervised_devloop(...)`
- proposal capture through `capture_from_proposal(...)`
- replay decision through `replay_or_fallback_for_run(...)`
- agent-facing feedback formatting

Use this first when you want one compact end-to-end sanity check.

## 2. Bootstrap and local promotion

Command:

```bash
cargo run -p evo_oris_repo --bin bootstrap_seed
```

Shows:

- `bootstrap_if_empty(...)` creating quarantined seed assets
- replay before local promotion (usually planner fallback)
- local capture that promotes a new capsule
- replay after local promotion (expected capsule reuse)

Use this when validating cold-start behavior and transition to reusable assets.

## 3. Supervised devloop policy gating

Command:

```bash
cargo run -p evo_oris_repo --bin supervised_devloop
```

Shows three outcomes in one run:

- `AwaitingApproval` when human approval is false
- `RejectedByPolicy` for out-of-scope path (not single `docs/*.md`)
- `Executed` for approved in-scope docs task

Use this for approval-flow and bounded-task-policy validation.

## 4. Spec-compiled mutation path

Command:

```bash
cargo run -p evo_oris_repo --bin spec_compiled_mutation
```

Shows:

- YAML spec parse + compile via `SpecCompiler`
- `prepare_mutation_from_spec(...)` capture path
- replay with `spec_id` selector narrowing

Use this when verifying spec-contract integration with evolution capture/replay.

## 5. Evolution network publish/import/fetch/revoke

Command:

```bash
cargo run -p evo_oris_repo --bin network_exchange
```

Shows:

- node A capture + `export_promoted_assets(...)`
- node B `import_remote_envelope(...)`
- `fetch_assets(...)` for signal-based retrieval
- `revoke_assets(...)` round-trip

Use this to validate distributed exchange primitives without deploying HTTP routes.

## 6. Economics stake gate

Command:

```bash
cargo run -p evo_oris_repo --bin economics_stake
```

Shows:

- publish rejection when EVU balance is insufficient
- successful publish after EVU funding and stake policy
- economics signal snapshot (`available_evu`, selector weight)

Use this to verify economics-based publish control.

## 7. Multi-agent coordination modes

Command:

```bash
cargo run -p evo_oris_repo --bin coordination_matrix
```

Shows:

- sequential coordination
- parallel coordination with fail-once retry behavior
- conditional coordination that skips downstream tasks after forced failure

Use this to inspect task dependency and coordination semantics.

## 8. Metrics and health surfaces

Command:

```bash
cargo run -p evo_oris_repo --bin metrics_health
```

Shows:

- replay attempts and successes
- promoted asset counters
- `health_snapshot(...)`
- Prometheus exposition via `render_metrics_prometheus(...)`

Use this for observability integration checks.

## 9. Deterministic signal extraction

Command:

```bash
cargo run -p evo_oris_repo --bin signal_extraction
```

Shows:

- deterministic token extraction from intent/diff/logs
- Rust error-code signal capture (`E0425` style)
- hash output for signal payload identity

Use this when tuning task-class matching quality.

## 10. Evo vs Non-Evo Benchmark

Command:

```bash
cargo run -p evo_oris_repo --bin evo_vs_non_evo_benchmark
```

Optional CLI:

```bash
cargo run -p evo_oris_repo --bin evo_vs_non_evo_benchmark -- \
  --planner openai-compatible \
  --iterations 10 \
  --model qwen3-235b-a22b \
  --planner-base-url https://mgallery.haier.net/v1 \
  --output-json target/evo_bench/report.json \
  --output-md target/evo_bench/report.md \
  --output-assets-json target/evo_bench/shareable_assets.json \
  --log-file target/evo_bench/benchmark.log \
  --allow-skip-non-evo true \
  --verbose
```

Minimal visibility check:

```bash
OPENAI_COMPAT_API_KEY=... cargo run -p evo_oris_repo --bin evo_vs_non_evo_benchmark -- \
  --planner openai-compatible \
  --iterations 1 \
  --model qwen3-235b-a22b \
  --planner-base-url https://mgallery.haier.net/v1 \
  --output-assets-json target/evo_bench/shareable_assets.json \
  --log-file target/evo_bench/benchmark.log \
  --allow-skip-non-evo true \
  --verbose
```

Local fast benchmark with Ollama:

```bash
ollama serve
ollama pull llama3
cargo run -p evo_oris_repo --bin evo_vs_non_evo_benchmark -- \
  --planner ollama \
  --iterations 10 \
  --model llama3 \
  --output-json target/evo_bench/report.json \
  --output-md target/evo_bench/report.md \
  --output-assets-json target/evo_bench/shareable_assets.json \
  --log-file target/evo_bench/benchmark.log \
  --allow-skip-non-evo true \
  --verbose
```

Shows:

- source agent creates 3 assets (docs/code/config) through capture + validation
- shareable asset targets are isolated under `examples/evo_oris_repo/assets/evo-shareable/`
- source assets are exported and imported by 2 consumer agents
- consumer agents replay imported assets to prove cross-agent reuse
- non-evo vs evo groups are compared on the same task set for token and duration
- planner provider is selectable (`openai-compatible`, `deepseek`, or local `ollama`)
- report is generated in JSON and Markdown
- shareable assets are also emitted as JSON manifest for direct inspection
- process logs are written to `target/evo_bench/benchmark.log` (or `--log-file`)

Output metrics:

- real model token usage from response usage fields
- offline token estimate (`cl100k_base`)
- success rate
- duration mean/p50/p95
- replay hit rate (evo group)
- reduction percentages when baseline exists

If planner credentials are missing with `--allow-skip-non-evo true`, baseline is marked `skipped_missing_key` and the report is still generated.

Evidence quick-check commands:

```bash
ls -la examples/evo_oris_repo/assets/evo-shareable/generated/run-*
jq . target/evo_bench/shareable_assets.json
tail -f target/evo_bench/benchmark.log
```

## 11. Recommended sequence

Run in this order for a complete local walkthrough:

1. `cargo run -p evo_oris_repo`
2. `cargo run -p evo_oris_repo --bin supervised_devloop`
3. `cargo run -p evo_oris_repo --bin bootstrap_seed`
4. `cargo run -p evo_oris_repo --bin spec_compiled_mutation`
5. `cargo run -p evo_oris_repo --bin network_exchange`
6. `cargo run -p evo_oris_repo --bin economics_stake`
7. `cargo run -p evo_oris_repo --bin coordination_matrix`
8. `cargo run -p evo_oris_repo --bin metrics_health`
9. `cargo run -p evo_oris_repo --bin signal_extraction`
10. `cargo run -p evo_oris_repo --bin evo_vs_non_evo_benchmark`

## 12. Validation commands

```bash
cargo check -p evo_oris_repo
cargo test -p oris-runtime --test evolution_feature_wiring --features full-evolution-experimental
```
