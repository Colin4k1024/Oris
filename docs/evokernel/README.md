# EvoKernel Design Mirrors

Local mirrors of the Notion design pages under:
https://www.notion.so/317e8a70eec5809c85e1f52aa03870e4

Last synced: March 5, 2026

This directory is both:

- a local mirror of the design set
- the practical entrypoint for the Evo features that are already wired in this repository

## Use the checked-in implementation first

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

What is still design-target only:

- always-on autonomous development loops
- automatic issue intake and task planning
- automatic branch, review, and release orchestration
- full remote network/economics/spec coordination beyond the current scaffolds

Read `../evokernel-v0.1.md` first for the architecture summary, then use the per-page mirrors below for deeper design details.
For runnable scenario coverage and output guidance, use `examples.md`.

Files:

- `architecture.md`
- `examples.md`
- `evolution.md`
- `governor.md`
- `network.md`
- `economics.md`
- `kernel.md`
- `implementation-roadmap.md`
- `bootstrap.md`
- `agent.md`
- `devloop.md`
- `spec.md`
- `vision.md`
- `founding-paper.md`

The top-level overview remains in `../evokernel-v0.1.md`.

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
