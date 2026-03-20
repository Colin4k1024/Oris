# Current Project Status

Last updated: March 20, 2026

## External Summary

Oris currently provides a supervised, bounded, and auditable self-evolution runtime. The checked-in repository supports replay-driven learning, bounded candidate selection, machine-readable mutation proposal contracts, replay-assisted supervised execution, acceptance gating, and bounded branch or pull-request artifact preparation. It does not yet claim an always-on autonomous software-improvement loop that independently discovers issues, plans work, merges code, publishes packages, or performs releases without explicit human approval.

## Current Product Statement

The accurate current product statement is:

> Oris supports a supervised closed-loop self-evolution path with bounded acceptance gating.

It is not yet accurate to say:

> Oris is a fully autonomous self-improving development and release system.

## What Is Checked In Today

- Replay-driven mutation capture and later reuse for the same or equivalent bounded task signals.
- Bounded issue candidate intake when a caller provides explicit issue metadata.
- Auditable mutation proposal contracts with expected evidence and approval requirements.
- Replay-assisted supervised execution with fail-closed fallback semantics.
- Acceptance gating that checks end-to-end evidence for internal consistency.
- Bounded delivery preparation that can emit branch and pull-request artifacts without merging or releasing them.
- Quarantined remote asset reuse that requires successful local validation before promotion.
- Full A2A protocol semantic surface (protocol core, task lifecycle, asset discovery, council/project workflows, economic lifecycle) through v0.61.0.
- Kernel 2.0 hardening: execution step contracts, canonical log store, replay cursor, interrupt objects, lease-based finalization, context-aware scheduler, backpressure engine.

## What Remains Out Of Scope

- Autonomous issue discovery without caller-provided candidates.
- Autonomous task planning outside the current bounded supervised path.
- Autonomous merge, publish, or release orchestration.
- Always-on unattended software improvement across arbitrary work classes.

## Evidence Basis

This status is based on the checked-in boundary and test gate described in:

- [self-evolution-acceptance-checklist.md](self-evolution-acceptance-checklist.md)
- [implementation-roadmap.md](implementation-roadmap.md)
- [README.md](README.md)
- [PROJECT_AUDIT_2026_Q1.md](../PROJECT_AUDIT_2026_Q1.md)

## Next Directions

The Q1 2026 audit identified five priority tracks for subsequent issues. See [PROJECT_AUDIT_2026_Q1.md](../PROJECT_AUDIT_2026_Q1.md) §8 for the full issue proposal table. Summary:

1. **Code Health (Track A):** Error handling cleanup in evokernel/runtime, debug output migration to tracing, deprecated code removal, test coverage expansion.
2. **Kernel Hardening (Track B):** Postgres backend parity, crash-recovery stress testing, lease terminal state persistence, scheduler fairness validation.
3. **Evolution Autonomy (Track C):** CI failure intake, confidence decay daemon, embedding-based task matching, agent feedback loop, bounded PR lane.
4. **Developer Experience (Track D):** Per-crate examples, quickstart tutorial, API reference docs.
5. **Production Readiness (Track E):** OpenTelemetry traces, Prometheus metrics, rate limiting, schema migration hardening.

## Reusable Release Note Paragraph

Oris currently ships a supervised, bounded self-evolution runtime rather than a fully autonomous software-improvement loop. In the checked-in repository, the system can capture successful mutations, replay them for later equivalent tasks, prepare auditable mutation proposals, execute a replay-assisted supervised path with fail-closed safety, and produce bounded branch or pull-request artifacts for review. Autonomous issue discovery, autonomous merge, publish, and release orchestration remain outside the current shipped boundary.