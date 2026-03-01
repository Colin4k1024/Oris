# Oris + Rust Ecosystem Integration Guide

This guide explains how to integrate Oris into common Rust stacks and how downstream users can adopt it safely.

## Integration principles

- Keep Oris execution contracts explicit (`thread_id`, idempotency keys, resume semantics).
- Treat persistence and replay as first-class concerns, not optional afterthoughts.
- Separate control-plane APIs (operators) from business APIs.
- Use structured logging and tracing from day one.

## Recommended integration patterns

## 1) Web backends (Axum/Actix)

- Use Oris runtime APIs behind authenticated service endpoints.
- Expose a health endpoint and keep Oris routes under a versioned prefix.
- Start from `examples/oris_starter_axum`.

## 2) Async runtime (Tokio)

- Run Oris inside Tokio services.
- Avoid blocking calls in async handlers.
- Use graceful shutdown to avoid interrupting in-flight work unexpectedly.

## 3) Persistence

- Local/dev:
  - `sqlite-persistence` for quick durable workflows.
- Production:
  - Use stable persistence strategy and plan migration/backup from the start.
  - Validate replay and recovery in CI.

## 4) Observability (tracing ecosystem)

- Use `tracing` and `tracing-subscriber` with request/run correlation IDs.
- Emit structured fields for:
  - `thread_id`/`run_id`
  - `attempt_id`
  - lease and interrupt identifiers
- Add metrics and traces before scaling worker concurrency.

## 5) CLI and operator tooling

- Pair service APIs with an operator CLI.
- Ensure CLI commands map one-to-one with API contracts:
  - run/list/inspect/resume/replay/cancel

## 6) Testing strategy

- Unit tests for reducers/state transitions.
- API tests for idempotency and conflict semantics.
- Crash/failover tests for checkpoint recovery and lease expiry.

## Adoption path for external users

1. Start with a single service and SQLite persistence.
2. Implement one business workflow graph end-to-end.
3. Add interrupt + resume flow with operator visibility.
4. Add replay and failover validation in CI.
5. Harden auth/security and observability before production rollout.

## Reference entry points

- Runtime examples: `crates/oris-runtime/examples/`
- Starter project: `examples/oris_starter_axum`
- Template matrix: `examples/templates/` (`cargo generate --path ... --name ...`)
- Durable execution docs: `docs/durable-execution.md`
- Kernel contract docs: `docs/kernel-api.md`

## Common mistakes to avoid

- Treating `thread_id` as optional/non-stable.
- Missing idempotency for run and step reporting.
- Skipping replay/recovery tests until late stages.
- Coupling business API errors with operator control-plane behavior.
