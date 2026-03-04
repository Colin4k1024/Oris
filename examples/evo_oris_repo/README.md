# Evo Oris Repo Example

This example demonstrates the current checked-in EvoKernel flow that already exists in this repository.

It is the fastest way to validate the latest experimental Evo surface without setting up the HTTP execution server.

## What it exercises

```text
AgentTask
-> MutationProposal
-> EvoKernel::capture_from_proposal
-> EvoKernel::feedback_for_agent
-> EvoKernel::replay_or_fallback_for_run
```

The example wires:

- `LocalProcessSandbox`
- `CommandValidator`
- `ValidationPlan`
- `JsonlEvolutionStore`
- `DefaultGovernor`

The example uses an explicit replay run id so `CapsuleReused.replay_run_id` stays attributable to the current replay execution while preserving the original capsule run id.

## Run

From repository root:

```bash
cargo run -p evo_oris_repo
```

Targeted smoke test:

```bash
cargo test -p oris-runtime --test evolution_feature_wiring --features full-evolution-experimental
```

## What it is not

- not an always-on autonomous development loop
- not an issue planner
- not an automatic branch/review/release system

For the design overview and implementation status, see:

- `docs/evokernel/README.md`
- `docs/evokernel-v0.1.md`
- `docs/evokernel/devloop.md`
