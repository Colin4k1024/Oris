# Replay Feedback ROI Stability Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Eliminate replay ROI drift between `metrics_snapshot()` and release-gate summaries by routing both through one deterministic aggregation path that preserves legacy fallback behavior.

**Architecture:** Keep `ReplayEconomicsRecorded` as the canonical replay-economics source, but centralize event-to-summary aggregation inside `crates/oris-evokernel/src/core.rs`. Reuse that helper from both snapshot and release-gate code paths so the same event window yields the same totals, then lock the contract with evokernel and runtime regression tests.

**Tech Stack:** Rust, `tokio`, cargo test/fmt, GitHub issue workflow.

Execution discipline: `@test-driven-development`, `@verification-before-completion`.

---

### Task 1: Capture the Legacy Drift with a Failing Evokernel Test

**Files:**
- Modify: `crates/oris-evokernel/src/core.rs`
- Test: `crates/oris-evokernel/src/core.rs`

**Step 1: Write the failing test**

Add a regression that creates a replay history with `CapsuleReused` and replay
validation failure events but no `ReplayEconomicsRecorded` entries, then assert
that the snapshot and release-gate summary agree on the same totals.

```rust
#[tokio::test]
async fn replay_roi_release_gate_summary_matches_metrics_snapshot_for_legacy_replay_history() {
    let evo = test_kernel().await;
    // seed promoted gene and replay history without ReplayEconomicsRecorded
    let metrics = evo.metrics_snapshot().unwrap();
    let summary = evo.replay_roi_release_gate_summary(0).unwrap();

    assert_eq!(summary.replay_attempts_total, metrics.replay_attempts_total);
    assert_eq!(summary.replay_success_total, metrics.replay_success_total);
    assert_eq!(summary.replay_failure_total, metrics.replay_attempts_total - metrics.replay_success_total);
    assert_eq!(summary.reasoning_avoided_tokens_total, metrics.reasoning_avoided_tokens_total);
    assert_eq!(summary.replay_fallback_cost_total, metrics.replay_fallback_cost_total);
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p oris-evokernel replay_roi_release_gate_summary_matches_metrics_snapshot_for_legacy_replay_history -- --nocapture`
Expected: FAIL because `metrics_snapshot()` uses a legacy fallback path and `replay_roi_release_gate_summary()` currently returns zeros without economics events.

**Step 3: Commit**

Do not commit yet. Move directly to the minimal implementation once the failure is confirmed.

### Task 2: Extract a Shared Replay ROI Aggregator and Make the Test Pass

**Files:**
- Modify: `crates/oris-evokernel/src/core.rs`
- Test: `crates/oris-evokernel/src/core.rs`

**Step 1: Write minimal implementation**

Introduce a shared helper that aggregates replay totals for both current and
legacy histories.

```rust
struct ReplayRoiAggregate {
    replay_attempts_total: u64,
    replay_success_total: u64,
    replay_failure_total: u64,
    reasoning_avoided_tokens_total: u64,
    replay_fallback_cost_total: u64,
    replay_task_classes: Vec<ReplayTaskClassMetrics>,
    replay_sources: Vec<ReplaySourceRoiMetrics>,
}

fn collect_replay_roi_aggregate(
    events: &[StoredEvolutionEvent],
    projection: &EvolutionProjection,
    cutoff: Option<DateTime<Utc>>,
) -> ReplayRoiAggregate {
    // prefer ReplayEconomicsRecorded when present in scope
    // otherwise reuse current legacy CapsuleReused / validation-failure fallback
}
```

Wire both `evolution_metrics_snapshot()` and
`replay_roi_release_gate_summary()` through that helper instead of maintaining
separate aggregation loops.

**Step 2: Run test to verify it passes**

Run: `cargo test -p oris-evokernel replay_roi_release_gate_summary_matches_metrics_snapshot_for_legacy_replay_history -- --nocapture`
Expected: PASS.

**Step 3: Refactor**

Remove duplicated task/source aggregation logic from the two call sites while
preserving current JSON field names and stable `BTreeMap` ordering.

**Step 4: Run focused evokernel summary coverage**

Run: `cargo test -p oris-evokernel --lib replay_roi_release_gate_summary_ -- --nocapture`
Expected: PASS, including existing ROI summary and machine-readable stability tests.

### Task 3: Lock Runtime Contract Consistency

**Files:**
- Modify: `crates/oris-runtime/tests/agent_self_evolution_travel_network.rs`
- Test: `crates/oris-runtime/tests/agent_self_evolution_travel_network.rs`

**Step 1: Write the failing test assertion**

Strengthen the existing travel-network scenario so release-gate contract input
must match the summary totals produced from the same window.

```rust
assert_eq!(release_gate_contract.input.replay_attempts_total, roi_summary.replay_attempts_total);
assert_eq!(release_gate_contract.input.replay_success_total, roi_summary.replay_success_total);
assert_eq!(release_gate_contract.input.replay_failure_total, roi_summary.replay_failure_total);
assert_eq!(release_gate_contract.input.replay_fallback_cost_total, roi_summary.replay_fallback_cost_total);
```

**Step 2: Run test to verify behavior**

Run: `cargo test -p oris-runtime --test agent_self_evolution_travel_network --features full-evolution-experimental -- --nocapture`
Expected: PASS once the shared evokernel aggregation is wired correctly.

**Step 3: Commit**

```bash
git add crates/oris-evokernel/src/core.rs crates/oris-runtime/tests/agent_self_evolution_travel_network.rs docs/plans/2026-03-14-replay-feedback-roi-stability-design.md docs/plans/2026-03-14-replay-feedback-roi-stability-implementation-plan.md
git commit -m "feat(evokernel): stabilize replay roi aggregation"
```

### Task 4: Run Feature-Class Validation Floor

**Files:**
- Modify: none
- Test: workspace validation commands

**Step 1: Run formatter check**

Run: `cargo fmt --all -- --check`
Expected: PASS.

**Step 2: Run workspace and release validation**

Run: `cargo test --workspace`
Expected: PASS.

Run: `cargo build --verbose --all --release --all-features`
Expected: PASS.

Run: `cargo test --release --all-features`
Expected: PASS.

**Step 3: Release readiness note**

If all commands pass, proceed into the maintainer release workflow for the issue:
version bump, release note, publish dry-run, publish, status transitions, and
issue closeout.
