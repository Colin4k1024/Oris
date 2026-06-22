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

- `evolution` exposes the standard supervised `oris_runtime::evolution` surface.
- `governor` exposes policy-only promotion, revocation, and cooldown decisions.
- `agent-contract` exposes the proposal-only external agent contract surface.
- `evolution-network` exposes the network facade; `a2a-production` keeps only the stable `/a2a/*` route subset.
- `task-class-toml` enables TOML-backed task-class loading in `oris-evolution`.
- `full-evolution-experimental` matches the repository example and test surface by additionally exposing `economics`, `spec_contract`, and wider experimental network routes.
- Legacy `*-experimental` feature names remain available as compatibility aliases during the migration window.

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
| [current-project-status.md](current-project-status.md) | External-facing current capability statement |

See [evokernel-v0.1.md](../evokernel-v0.1.md) for architecture overview.

## Implementation Status Matrix

| Layer | Local crate/module | Status | Gate |
| --- | --- | --- | --- |
| Kernel | `crates/oris-kernel` | implemented baseline | default |
| Evolution | `crates/oris-evolution` | standard supervised baseline with extended lifecycle primitives | `evolution` |
| Sandbox | `crates/oris-sandbox` | standard supervised execution baseline, blast radius helper added | `evolution` |
| EvoKernel | `crates/oris-evokernel` | standard supervised baseline, governor-aware capture added | `evolution` |
| Governor | `crates/oris-governor` | standard policy-only decision surface | `governor` |
| Evolution Network | `crates/oris-evolution-network` | standard facade entrypoint; wider publish/fetch/revoke routes remain experimental outside `a2a-production` | `evolution-network` |
| Economics | `crates/oris-economics` | in progress, experimental ledger scaffold | `economics-experimental` |
| Spec | `crates/oris-spec` | in progress, experimental YAML compiler scaffold | `spec-experimental` |
| Agent Contract | `crates/oris-agent-contract` | standard proposal-only contract surface | `agent-contract` |
| Full stack | `crates/oris-runtime` re-exports | experimental aggregate | `full-evolution-experimental` |

The `Gate` column shows the narrowest recommended module-level flag. Use `full-evolution-experimental` only when you want the checked-in example and the full experimental facade bundle exposed through `oris-runtime` together.

Pages marked `In Progress` describe the target design and now include implementation snapshots where the current crate only exposes a subset of the planned behavior.
