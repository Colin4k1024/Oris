# Oris

**Oris is an AI self-evolution framework for supervised, bounded, closed-loop software improvement.**

[![Latest Version](https://img.shields.io/crates/v/oris-runtime.svg)](https://crates.io/crates/oris-runtime)
[![docs.rs](https://img.shields.io/docsrs/oris-runtime)](https://docs.rs/oris-runtime)
[![codecov](https://codecov.io/gh/Colin4k1024/Oris/graph/badge.svg)](https://codecov.io/gh/Colin4k1024/Oris)

---

## Why Oris

Most systems can execute tasks, but cannot systematically improve how they solve recurring problems.

Oris focuses on **closed-loop improvement** for AI software:

- Capture real signals from failures and regressions.
- Generate and validate candidate mutations safely.
- Promote proven solutions into reusable assets.
- Reuse successful solutions with confidence-aware replay.

Current checked-in boundary:

- Supervised, bounded, and auditable self-evolution.
- Experience capture, replay reuse, and fail-closed policy enforcement.
- No claim of fully autonomous issue discovery, merge, publish, or release.

See [the current project status note](docs/evokernel/current-project-status.md) for a concise external-facing statement of the shipped boundary.

---

## Capability Boundary

| In Scope (Primary) | Supporting Layer | Not Primary in This README |
|---|---|---|
| Self-evolution loop and framework primitives | Durable execution and orchestration runtime | Exhaustive runtime API reference |
| Gene/Capsule lifecycle and confidence reuse | Storage/checkpoint backends and deployment integration | Detailed access policy, endpoint, and metrics contracts |
| Evolution-oriented scenario workflows | Production operations and integration surface | General-purpose workflow engine positioning |

---

## Self-Evolution Loop

The current implementation supports a **supervised closed-loop self-evolution path with bounded acceptance gating**. It does not yet claim a fully autonomous self-evolving agent or always-on autonomous release loop.

Oris implements an 8-stage self-evolution loop:

1. **Detect** — collect actionable signals from compiler/test/runtime outcomes.
2. **Select** — choose the best candidate gene or strategy.
3. **Mutate** — generate candidate changes from prior successful patterns.
4. **Execute** — run mutations in a controlled sandbox.
5. **Validate** — verify correctness and safety gates.
6. **Evaluate** — compare improvement versus regression.
7. **Solidify** — promote successful mutations into durable assets.
8. **Reuse** — replay proven assets with confidence tracking.

---

## EvoMap Alignment

Oris maps EvoMap concepts to concrete framework behavior:

| EvoMap Concept | Oris Mapping |
|---|---|
| Worker Pool | `EvolutionPipeline` stages |
| Task Queue | Signal intake and selection flow |
| Bounty System | Issue intake and prioritization |
| A2A Protocol | `oris-evolution-network` experimental protocol |

See [EvoMap alignment details](docs/evomap-vs-oris-comparison.md).

---

## What You Can Build

- Self-improving AI agents that learn from failed runs.
- Supervised dev loops for bounded recurring issues.
- Evolution-aware replay pipelines with confidence lifecycle.
- Cross-agent knowledge exchange over an evolution network surface.

---

## Quick Start

Install the core crate and enable the framework surface:

```bash
cargo add oris-runtime
cargo add oris-runtime --features full-evolution-experimental
export OPENAI_API_KEY="your-key"
```

Run the canonical evolution scenario:

```bash
cargo run -p evo_oris_repo
```

Run the first-run script with observable artifacts:

```bash
bash scripts/evo_first_run.sh
```

Expected outputs:

- `target/evo_first_run/summary.json`
- `target/evo_first_run/run.log`

---

## Components & Maturity

Maturity below reflects the current checked-in framework surface.

| Component | Crate | Maturity | Gate |
|---|---|---|---|
| Evolution Core | `crates/oris-evolution` | Implemented baseline with extended lifecycle primitives | `evolution-experimental` |
| Sandbox | `crates/oris-sandbox` | Implemented baseline | `evolution-experimental` |
| EvoKernel | `crates/oris-evokernel` | Implemented baseline with governor-aware capture | `evolution-experimental` |
| Intake | `crates/oris-intake` | Implemented baseline for issue intake/prioritization | `intake-experimental` |
| Evolution Network | `crates/oris-evolution-network` | Experimental protocol scaffold | `evolution-network-experimental` |
| Experience Repository | `crates/oris-experience-repo` | v0.3.0 — Ed25519 signature verification fully enabled, PKI key registry, rate limiting on all endpoints | standalone crate |
| Full Framework Facade | `crates/oris-runtime` re-exports | Aggregate framework surface | `full-evolution-experimental` |

---

## Runtime Integration (Brief)

The runtime layer is a **supporting integration surface** for hosting and operating the framework (execution server, workers, durable jobs). This README does not act as a runtime handbook; use the docs below when you need runtime-level details.

- [Production operations guide](docs/production-operations-guide.md)
- [Starter Axum integration example](examples/oris_starter_axum/README.md)
- [Runtime API contract](docs/runtime-api-contract.json)

---

## Learn More

- [EvoKernel docs index](docs/evokernel/README.md)
- [Evolution example suite](examples/evo_oris_repo/README.md)
- [Production operations guide](docs/production-operations-guide.md)
- [Evo example programs](docs/evokernel/examples.md)
- [EvoKernel overview](docs/evokernel-v0.1.md)

---

## Community / License

- License: [MIT](LICENSE)
- Attribution: This project includes code derived from [langchain-rust](https://github.com/langchain-ai/langchain-rust).
- Contribution guide: [CONTRIBUTING.md](CONTRIBUTING.md)
- Code of conduct: [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md)
- Security policy: [SECURITY.md](SECURITY.md)
- Privacy notice: [PRIVACY.md](PRIVACY.md)
- Support guide: [SUPPORT.md](SUPPORT.md)
- Governance: [GOVERNANCE.md](GOVERNANCE.md)
- Crate: [crates.io/oris-runtime](https://crates.io/crates/oris-runtime)
- API docs: [docs.rs/oris-runtime](https://docs.rs/oris-runtime)
- Repository: [GitHub](https://github.com/Colin4k1024/Oris)
