# Continuous Confidence Control Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Unify confidence decay, revalidation, and revocation evidence for self-evolution assets and keep the runtime experimental API surface aligned with the lifecycle contracts.

**Architecture:** Refactor `oris-evokernel` confidence transitions around shared evidence helpers, prove behavior with regression tests first, and extend `oris-runtime` feature wiring to lock the exported transition types in place.

**Tech Stack:** Rust, `tokio`, `cargo test`, Oris evolution/evokernel/runtime crates.

Execution discipline: `@test-driven-development`, `@verification-before-completion`.

---

### Task 1: Add Failing Regression for Confidence Lifecycle Evidence

**Files:**
- Modify: `crates/oris-evokernel/tests/evolution_lifecycle_regression.rs`

**Step 1: Write the failing test**

Add a regression that seeds a stale promoted asset, triggers revalidation, and asserts the resulting `PromotionEvaluated` event carries stable evidence fields and reason code.

**Step 2: Run test to verify it fails**

Run: `cargo test -p oris-evokernel --test evolution_lifecycle_regression <new_test_name> -- --nocapture`

**Step 3: Write minimal implementation**

Implement only the confidence evidence normalization needed for the new test.

**Step 4: Run test to verify it passes**

Run the same targeted command and confirm PASS.

**Step 5: Commit**

```bash
git add crates/oris-evokernel/tests/evolution_lifecycle_regression.rs crates/oris-evokernel/src/core.rs
git commit -m "feat(evokernel): unify confidence lifecycle evidence"
```

### Task 2: Extend Confidence Transition Helpers

**Files:**
- Modify: `crates/oris-evokernel/src/core.rs`
- Modify: `crates/oris-evolution/src/core.rs` (only if helper data needs to stay aligned)

**Step 1: Write the failing test**

Add or refine a helper-level test that proves stale confidence targets and replay-failure revocation produce consistent summary/evidence values.

**Step 2: Run test to verify it fails**

Run: `cargo test -p oris-evokernel --lib confidence -- --nocapture`

**Step 3: Write minimal implementation**

Extract shared confidence transition evidence construction and reuse it in both revalidation and revocation paths.

**Step 4: Run tests to verify pass**

Run:
- `cargo test -p oris-evokernel --lib -- --nocapture`
- `cargo test -p oris-evokernel --test evolution_lifecycle_regression -- --nocapture`

**Step 5: Commit**

```bash
git add crates/oris-evokernel/src/core.rs crates/oris-evolution/src/core.rs crates/oris-evokernel/tests/evolution_lifecycle_regression.rs
git commit -m "feat(evokernel): harden confidence decay and revocation lifecycle"
```

### Task 3: Lock Runtime Wiring for Confidence Transition Types

**Files:**
- Modify: `crates/oris-runtime/tests/evolution_feature_wiring.rs`

**Step 1: Write the failing test**

Extend the existing feature wiring test to require the transition reason/evidence types used by confidence lifecycle handling.

**Step 2: Run test to verify it fails**

Run: `cargo test -p oris-runtime --test evolution_feature_wiring --features full-evolution-experimental -- --nocapture`

**Step 3: Write minimal implementation**

Adjust runtime re-exports only if the new assertions expose a missing path.

**Step 4: Run tests to verify pass**

Run the same runtime command and confirm PASS.

**Step 5: Commit**

```bash
git add crates/oris-runtime/tests/evolution_feature_wiring.rs crates/oris-runtime/src/*.rs
 git commit -m "test(runtime): lock confidence transition feature wiring"
```
