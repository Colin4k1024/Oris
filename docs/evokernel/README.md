# EvoKernel — Self-Evolution Runtime

> **Oris provides a supervised, bounded self-evolution runtime.**

EvoKernel is the self-evolution subsystem of Oris, providing:

- **Signal Extraction** — Detect problems from runtime (compiler, panics, tests)
- **Gene Selection** — Choose best candidates from the gene pool
- **Mutation Pipeline** — 8-stage evolution pipeline (Detect → Select → Mutate → Execute → Validate → Evaluate → Solidify → Reuse)
- **Confidence Lifecycle** — Automatic decay/boost based on reuse success
- **Issue Intake** — Automated problem detection and prioritization

Current checked-in boundary:

- Supervised, bounded, and auditable self-evolution flows.
- Experience capture, replay reuse, and fail-closed policy enforcement.
- No claim of always-on autonomous issue discovery, merge, publish, or release.

## Quick Start

If you want to exercise the latest code that exists in this repository today, start with the checked-in example suite and smoke test:

```bash
cargo add oris-runtime --features full-evolution-experimental
cargo run -p evo_oris_repo
cargo run -p evo_oris_repo --bin supervised_devloop
cargo run -p evo_oris_repo --bin network_exchange
cargo test -p oris-runtime --test evolution_feature_wiring --features full-evolution-experimental
```

Feature guide:

- `evolution-experimental` exposes `oris_runtime::evolution` only.
- `full-evolution-experimental` matches the repository example and test surface by additionally exposing `governor`, `evolution_network`, `economics`, `spec_contract`, and `agent_contract`.

The current repository-backed flow is:

```text
AgentTask
-> MutationProposal
-> EvoKernel::capture_from_proposal
-> EvoKernel::feedback_for_agent
-> EvoKernel::replay_or_fallback_for_run
```

Use `EvoKernel::replay_or_fallback_for_run(...)` when replay events should be tied to a caller-controlled run id. `EvoKernel::replay_or_fallback(...)` remains available and generates a replay run id automatically.

What this already covers:

- proposal-driven mutation capture
- sandboxed command validation via `LocalProcessSandbox` and `CommandValidator`
- `ValidationPlan`-based verification stages
- append-only JSONL evolution storage via `JsonlEvolutionStore`
- replay-first candidate lookup through the selector path
- supervised closed-loop self-evolution for a bounded subset of development work

**Implemented Features:**

- ✅ Proposal-driven mutation capture
- ✅ Sandboxed command validation (`LocalProcessSandbox`, `CommandValidator`)
- ✅ `ValidationPlan`-based verification stages
- ✅ Append-only JSONL evolution storage (`JsonlEvolutionStore`)
- ✅ Replay-first candidate lookup through selector path
- ✅ 8-stage EvolutionPipeline
- ✅ Confidence lifecycle with decay/boost
- ✅ Signal extraction from runtime
- ✅ Issue intake with priority scoring

**In Development:**

- Broader autonomous issue discovery and task planning
- Stronger always-on agent orchestration beyond the supervised bounded path
- Autonomous branch, review, publish, and release orchestration

## Documentation Index

| Document | Description |
|----------|-------------|
| [architecture.md](architecture.md) | System architecture |
| [evolution.md](evolution.md) | Gene/Capsule lifecycle |
| [pipeline.md](pipeline.md) | 8-stage evolution pipeline |
| [confidence.md](confidence.md) | Confidence lifecycle |
| [governor.md](governor.md) | Evolution governance |
| [network.md](network.md) | A2A protocol alignment |
| [intake.md](intake.md) | Issue intake system |
| [examples.md](examples.md) | Runnable examples |

See [evokernel-v0.1.md](../evokernel-v0.1.md) for architecture overview.

## Implementation Status Matrix

| Layer | Local crate/module | Status | Gate |
| --- | --- | --- | --- |
| Kernel | `crates/oris-kernel` | implemented baseline | default |
| Evolution | `crates/oris-evolution` | in progress, implemented baseline with extended lifecycle primitives | `evolution-experimental` |
| Sandbox | `crates/oris-sandbox` | implemented baseline, blast radius helper added | `evolution-experimental` |
| EvoKernel | `crates/oris-evokernel` | implemented baseline, governor-aware capture added | `evolution-experimental` |
| Governor | `crates/oris-governor` | in progress, experimental scaffold with default policy | `governor-experimental` |
| Evolution Network | `crates/oris-evolution-network` | in progress, experimental protocol scaffold | `evolution-network-experimental` |
| Economics | `crates/oris-economics` | in progress, experimental ledger scaffold | `economics-experimental` |
| Spec | `crates/oris-spec` | in progress, experimental YAML compiler scaffold | `spec-experimental` |
| Agent Contract | `crates/oris-agent-contract` | in progress, experimental proposal contract scaffold | `agent-contract-experimental` |
| Full stack | `crates/oris-runtime` re-exports | experimental aggregate | `full-evolution-experimental` |

The `Gate` column shows the narrowest module-level flag. Use `full-evolution-experimental` when you want the checked-in example and the full facade bundle exposed through `oris-runtime` together.

Pages marked `In Progress` describe the target design and now include implementation snapshots where the current crate only exposes a subset of the planned behavior.
