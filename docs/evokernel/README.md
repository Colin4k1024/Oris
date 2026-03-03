# EvoKernel Design Mirrors

Local mirrors of the Notion design pages under:
https://www.notion.so/317e8a70eec5809c85e1f52aa03870e4

Last synced: March 3, 2026

Files:

- `architecture.md`
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

Pages marked `In Progress` describe the target design and now include implementation snapshots where the current crate only exposes a subset of the planned behavior.
