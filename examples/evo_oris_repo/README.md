# Evo Oris Repo Example Suite

This package is the runnable Evolution scenario suite for the current checked-in Oris repo.

It expands the original canonical flow into multiple focused programs so each major Evo capability can be validated independently.

## Canonical First Run

Before using the full scenario matrix, run the repository-level onboarding entry:

```bash
bash scripts/evo_first_run.sh
```

This is the default contributor entrypoint and produces:

- `target/evo_first_run/summary.json`
- `target/evo_first_run/run.log`

## Prerequisites

From repository root:

```bash
cargo add oris-runtime --features full-evolution-experimental
```

## Scenario Matrix

| Program | Command | What it demonstrates |
| --- | --- | --- |
| Canonical capture/replay | `cargo run -p evo_oris_repo` | `AgentTask -> MutationProposal -> run_supervised_devloop/capture_from_proposal -> replay_or_fallback_for_run` |
| Bootstrap to local promotion | `cargo run -p evo_oris_repo --bin bootstrap_seed` | `bootstrap_if_empty`, replay before/after local promoted capture |
| Supervised devloop policy gate | `cargo run -p evo_oris_repo --bin supervised_devloop` | `AwaitingApproval`, `RejectedByPolicy`, `Executed` paths |
| Spec-compiled mutation | `cargo run -p evo_oris_repo --bin spec_compiled_mutation` | `SpecCompiler` + `prepare_mutation_from_spec` + spec-linked replay |
| Network exchange | `cargo run -p evo_oris_repo --bin network_exchange` | `export_promoted_assets`, `import_remote_envelope`, `fetch_assets`, `revoke_assets` |
| Economics stake gate | `cargo run -p evo_oris_repo --bin economics_stake` | EVU insufficiency rejection and successful publish after balance/stake policy |
| Coordination matrix | `cargo run -p evo_oris_repo --bin coordination_matrix` | Sequential/Parallel/Conditional multi-agent coordination outcomes |
| Metrics and health | `cargo run -p evo_oris_repo --bin metrics_health` | `metrics_snapshot`, `health_snapshot`, Prometheus rendering |
| Deterministic signal extraction | `cargo run -p evo_oris_repo --bin signal_extraction` | `extract_deterministic_signals` tokenization and hash output |
| Evo vs non-evo benchmark | `cargo run -p evo_oris_repo --bin evo_vs_non_evo_benchmark` | Creates 3 assets, shares to 2 consumers, compares non-evo vs evo token/time over repeated runs |

## Benchmark Scenario

Default benchmark run:

```bash
cargo run -p evo_oris_repo --bin evo_vs_non_evo_benchmark
```

Custom run:

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

Minimal visibility check (assets + manifest first):

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

Local fast run with Ollama (`llama3`):

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

Behavior:

- Creates 3 mixed-task assets (`docs`, `code`, `config`) on source agent.
- Shareable benchmark asset targets are isolated under `examples/evo_oris_repo/assets/evo-shareable/`.
- Shares assets to 2 consumer agents through export/import envelope.
- Runs 10-iteration comparison:
  - `non_evo`: planner model selection each task, then capture + validation.
  - `evo`: replay-first; fallback to planner only on replay miss.
- `--planner` can switch planning model provider:
  - `openai-compatible` uses `OPENAI_COMPAT_API_KEY` + `--planner-base-url`.
  - `deepseek` uses `DEEPSEEK_API_KEY`.
  - `ollama` uses local Ollama model (for example `llama3`).
- Records dual token metrics:
  - real token usage from model response usage fields.
  - offline token estimate with `cl100k_base`.
- Outputs both JSON and Markdown reports.
- Also writes shareable asset manifest JSON (`target/evo_bench/shareable_assets.json`) so you can inspect exported asset IDs, capsule IDs, and signals directly.
- Writes detailed process log to `target/evo_bench/benchmark.log` (or `--log-file`).
- If planner credentials are missing and `--allow-skip-non-evo true`, baseline group is skipped and report still generated.

How to inspect evidence quickly:

```bash
ls -la examples/evo_oris_repo/assets/evo-shareable/generated/run-*
jq . target/evo_bench/shareable_assets.json
tail -f target/evo_bench/benchmark.log
```

## Common Validation

Targeted compile check for this package:

```bash
cargo check -p evo_oris_repo
```

Repo-level Evo smoke test:

```bash
cargo test -p oris-runtime --test evolution_feature_wiring --features full-evolution-experimental
```

## Notes

- These examples execute mutations only inside sandbox copies, not your working tree.
- Evolution modules remain experimental and feature-gated.
- The suite focuses on bounded, inspectable scenarios, not an always-on autonomous issue-to-release loop.

For broader design and strategy context:

- `docs/evokernel/README.md`
- `docs/evokernel/examples.md`
- `docs/ORIS_2.0_STRATEGY.md`
