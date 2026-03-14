# Continuous Confidence Control Design

Date: 2026-03-14
Status: Approved for implementation
Issue: #229

## Objective

Harden the self-evolution confidence lifecycle so stale or high-risk assets are deterministically revalidated, demoted, or revoked with consistent reason codes and evidence summaries across events, metrics, and exported runtime APIs.

## Baseline

Current behavior already has two partially overlapping paths:

- stale promoted assets are quarantined by `apply_confidence_revalidation()` when decayed confidence falls below `MIN_REPLAY_CONFIDENCE`
- replay-failure regression can revoke assets through governor decisions during replay validation

These paths both emit `PromotionEvaluated` events, but they do not share a single confidence transition model, and the API exposure for transition reason/evidence types is not explicitly wired in runtime feature tests.

## Recommended Approach

1. Centralize confidence transition evidence building in `oris-evokernel` so decay-driven revalidation and replay-failure demotion/revocation produce the same evidence shape.
2. Preserve deterministic reason codes for each lifecycle outcome (`revalidation`, `downgrade`, `revocation`) and ensure metrics/events continue to count them consistently.
3. Extend runtime feature wiring coverage so confidence transition evidence/reason types remain visible through the experimental runtime surface.

## Scope

In scope:

- `oris-evokernel` confidence revalidation and replay-failure state transitions
- event/evidence consistency for confidence lifecycle transitions
- regression coverage in `evolution_lifecycle_regression.rs`
- runtime experimental wiring exposure checks

Out of scope:

- changing confidence decay constants
- redesigning governor policy thresholds
- adding new network/API endpoints

## Testing Strategy

- TDD on `crates/oris-evokernel/tests/evolution_lifecycle_regression.rs`
- keep `cargo test -p oris-evokernel --lib` green for confidence helpers and metrics
- extend `crates/oris-runtime/tests/evolution_feature_wiring.rs` for public type exposure

## Acceptance Criteria

- stale assets trigger deterministic confidence revalidation with auditable evidence summary
- replay-failure demotion/revocation uses the same evidence conventions and stable reason codes
- exported runtime experimental APIs surface the confidence transition types required for downstream inspection
