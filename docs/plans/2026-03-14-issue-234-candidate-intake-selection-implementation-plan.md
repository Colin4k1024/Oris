# Issue 234 Candidate Intake and Selection Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add a GitHub issue-shaped self-evolution candidate intake and selection contract with fail-closed decision semantics.

**Architecture:** Introduce a small selection-stage contract in `oris-agent-contract` and a dedicated `select_self_evolution_candidate(...)` method in `oris-evokernel`. Keep the selection boundary aligned to the existing docs-only bounded task classes and lock the public surface via runtime wiring tests.

**Tech Stack:** Rust, serde, oris-agent-contract, oris-evokernel, oris-runtime integration tests

---

### Task 1: Add failing contract and selection tests

**Files:**
- Modify: `crates/oris-evokernel/tests/evolution_lifecycle_regression.rs`
- Modify: `crates/oris-runtime/tests/evolution_feature_wiring.rs`

**Step 1: Write the failing evokernel tests**

Add tests for:
- `candidate_intake_accepts_open_evolution_feature_docs_issue`
- `candidate_intake_rejects_closed_issue_fail_closed`
- `candidate_intake_rejects_missing_evolution_label`
- `candidate_intake_rejects_unsupported_scope`

Each test should assert:
- `selected`
- `candidate_class`
- `reason_code`
- `failure_reason` / `recovery_hint` on rejects
- `fail_closed`

**Step 2: Write the failing runtime wiring test**

Assert the runtime facade exports:
- `SelfEvolutionCandidateIntakeRequest`
- `SelfEvolutionSelectionReasonCode`
- `SelfEvolutionSelectionDecision`
- `EvoKernel::select_self_evolution_candidate`

**Step 3: Run the targeted tests to verify RED**

Run:
- `cargo test -p oris-evokernel --test evolution_lifecycle_regression candidate_intake_ -- --nocapture`
- `cargo test -p oris-runtime --test evolution_feature_wiring --features full-evolution-experimental -- --nocapture`

Expected:
- failures because the new contract types and method do not exist yet

### Task 2: Add minimal agent-contract support

**Files:**
- Modify: `crates/oris-agent-contract/src/lib.rs`

**Step 1: Add the intake request struct**

Include:
- `issue_number`
- `title`
- `body`
- `labels`
- `state`
- `candidate_hint_paths`

**Step 2: Add the reason-code enum and decision struct**

Include stable serde-friendly types for accept and reject output.

**Step 3: Add a small normalization helper for reject decisions**

Map each reason code to default:
- failure reason
- recovery hint
- fail-closed behavior

**Step 4: Run the targeted tests again**

Run the same targeted test commands.

Expected:
- tests still fail, now because evokernel behavior is missing

### Task 3: Implement the evokernel selection method

**Files:**
- Modify: `crates/oris-evokernel/src/core.rs`

**Step 1: Add a helper that normalizes issue labels and state**

Behavior:
- compare case-insensitively
- treat unknown state values as fail-closed reject inputs

**Step 2: Reuse bounded docs-scope classification**

Use the existing docs normalization logic to classify `candidate_hint_paths` into `BoundedTaskClass`.

**Step 3: Add `select_self_evolution_candidate(...)`**

Behavior:
- accept only `OPEN` issues labeled `area/evolution` and `type/feature`
- reject `duplicate`, `invalid`, `wontfix`
- reject unsupported file scope
- return machine-readable accept or reject decisions with stable reason codes and summaries

**Step 4: Run the targeted tests to verify GREEN**

Run:
- `cargo test -p oris-evokernel --test evolution_lifecycle_regression candidate_intake_ -- --nocapture`
- `cargo test -p oris-runtime --test evolution_feature_wiring --features full-evolution-experimental -- --nocapture`

Expected:
- targeted tests pass

### Task 4: Refactor lightly and run full validation

**Files:**
- Modify only if cleanup is needed in files already touched

**Step 1: Run formatting check**

Run:
- `cargo fmt --all -- --check`

**Step 2: Run feature validation floor**

Run:
- `cargo test --workspace`
- `cargo build --verbose --all --release --all-features`
- `cargo test --release --all-features`

**Step 3: Commit the issue implementation**

Run:
- `git add crates/oris-agent-contract/src/lib.rs crates/oris-evokernel/src/core.rs crates/oris-evokernel/tests/evolution_lifecycle_regression.rs crates/oris-runtime/tests/evolution_feature_wiring.rs docs/plans/2026-03-14-issue-234-candidate-intake-selection-design.md docs/plans/2026-03-14-issue-234-candidate-intake-selection-implementation-plan.md`
- `git commit -m "feat(evokernel): add self-evolution candidate intake contracts"`

Expected:
- one clean implementation commit ready for release work
