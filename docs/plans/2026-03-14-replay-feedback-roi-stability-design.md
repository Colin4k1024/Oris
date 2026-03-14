# Replay Feedback ROI Stability Design

Date: 2026-03-14
Status: Approved for planning
Issue: `#230 [EVO26-W7-03][P1] Replay Feedback Loop Metrics and ROI Stability`

## 1. Objective

Keep replay feedback metrics, ROI summaries, and release-gate inputs stable and
comparable when they are derived from the same event history.

## 2. Baseline

Today `ReplayEconomicsRecorded` is the only event that carries explicit replay
ROI facts such as `reasoning_avoided_tokens`, `replay_fallback_cost`, and
`replay_roi`. The problem is not the event shape. The drift comes from having
multiple aggregators with different fallback behavior.

- `evolution_metrics_snapshot()` aggregates `ReplayEconomicsRecorded`, but when
  those events are absent it reconstructs replay totals from legacy
  `CapsuleReused` and replay validation-failure events.
- `replay_roi_release_gate_summary()` aggregates only
  `ReplayEconomicsRecorded` and has no equivalent fallback path.
- As a result, the same store can report non-zero replay metrics in
  `metrics_snapshot()` and zero replay evidence in the release-gate summary.

That violates the issue goal that release-gate metrics be repeatable and
comparable.

## 3. Decision

Use a single shared replay ROI aggregation path for both metrics snapshots and
release-gate summaries.

Chosen approach:
- keep `ReplayEconomicsRecorded` as the canonical fact source when present
- preserve the existing legacy fallback semantics for stores that predate replay
  economics events
- move the replay aggregation logic into one helper that both
  `evolution_metrics_snapshot()` and `replay_roi_release_gate_summary()` use

Rejected alternatives:
- new pre-aggregated events: faster reads but creates a second fact source and
  new drift risk
- test-only hardening: catches regressions later but leaves duplicate logic in
  place

## 4. Architecture

Add a shared internal replay aggregation helper in
`crates/oris-evokernel/src/core.rs` that accepts:
- the scanned event history
- the current projection when legacy replay reconstruction is needed
- an optional time cutoff for windowed summaries

The helper returns a single replay aggregate model containing:
- total attempts, successes, failures
- reasoning avoided tokens and fallback cost totals
- per-task-class aggregates
- per-source aggregates
- derived ROI values computed from those totals

Behavioral rules:
- if at least one `ReplayEconomicsRecorded` event exists in scope, aggregate only
  those economics events
- if no economics events exist in scope, reconstruct replay success/failure using
  the same legacy rules currently embedded in `evolution_metrics_snapshot()`
- continue using `BTreeMap` for task-class and source aggregation so output order
  stays stable across repeated runs

## 5. Data Semantics

The shared helper should preserve current published semantics:
- `replay_attempts_total = replay_success_total + replay_failure_total`
- `reasoning_steps_avoided_total` for task-class metrics remains derived from
  replay success count
- `replay_roi` remains `compute_replay_roi(reasoning_avoided_tokens_total,
  replay_fallback_cost_total)`
- remote-source metrics are emitted only when a replay economics event includes a
  `source_sender_id`

For legacy fallback windows, release-gate summaries should now mirror the same
success/failure and token-floor reconstruction already used by
`metrics_snapshot()`.

## 6. Testing Strategy

Add a regression that builds a legacy replay history without any
`ReplayEconomicsRecorded` events and proves that `metrics_snapshot()` and
`replay_roi_release_gate_summary(0)` report the same key totals.

Keep existing summary tests and extend runtime coverage so the travel-network
scenario asserts the release-gate contract input remains aligned with the shared
summary totals.

Required validation for this issue:
- `cargo test -p oris-evokernel --lib replay_roi_release_gate_summary_ -- --nocapture`
- `cargo test -p oris-runtime --test agent_self_evolution_travel_network --features full-evolution-experimental -- --nocapture`
- feature-class release floor from the maintainer matrix before publish

## 7. Scope Boundaries

In scope:
- replay ROI aggregation consistency
- deterministic ordering and comparable release-gate input
- regression coverage for legacy and current event histories

Out of scope:
- changing the public replay economics event schema
- introducing new release-gate thresholds
- expanding `ReplayFeedback` contract fields unless a failing test proves it is
  required

## 8. Acceptance Criteria

The design is complete when:
- `metrics_snapshot()` and `replay_roi_release_gate_summary()` cannot drift on the
  same replay history
- legacy stores without `ReplayEconomicsRecorded` still produce meaningful,
  comparable release-gate summaries
- repeated summary generation over the same event window remains stable except
  for `generated_at`
