# Federated Evolution Attribution and Revocation Hardening Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Harden federated remote attribution and revocation so remote ownership is consistently auditable and explicit revoke notices fail closed on any ownership mismatch.

**Architecture:** Keep `RemoteAssetImported` and replay economics events as the canonical facts, but centralize asset ownership reconstruction and revoke authorization inside `crates/oris-evokernel/src/core.rs`. Reuse that shared logic from both explicit revoke handling and replay-failure revocation evidence so import, economics, and negative-path transitions stay aligned.

**Tech Stack:** Rust, Tokio tests, `oris-evokernel`, `oris-runtime`, append-only evolution event store.

---

### Task 1: Lock the fail-closed revoke contract in evokernel tests

**Files:**
- Modify: `crates/oris-evokernel/tests/evolution_lifecycle_regression.rs`
- Test: `crates/oris-evokernel/tests/evolution_lifecycle_regression.rs`

**Step 1: Write the failing test**

Add regressions that cover:

- a remote owner successfully revoking its own imported assets
- a mixed-owner revoke notice failing closed without appending revoke/quarantine events

Use existing remote publish/import helpers instead of creating a new fixture stack.

**Step 2: Run test to verify it fails**

Run: `cargo test -p oris-evokernel --test evolution_lifecycle_regression remote_revoke_ -- --nocapture`
Expected: FAIL because the current revoke path does not enforce whole-request ownership authorization.

**Step 3: Write minimal implementation**

Implement shared revoke ownership validation in `crates/oris-evokernel/src/core.rs` and wire it into `revoke_assets_in_store(...)` before any events are appended.

**Step 4: Run test to verify it passes**

Run: `cargo test -p oris-evokernel --test evolution_lifecycle_regression remote_revoke_ -- --nocapture`
Expected: PASS

**Step 5: Commit**

```bash
git add crates/oris-evokernel/src/core.rs crates/oris-evokernel/tests/evolution_lifecycle_regression.rs
git commit -m "feat(evokernel): fail closed on mixed remote revoke ownership"
```

### Task 2: Lock remote replay-failure attribution evidence

**Files:**
- Modify: `crates/oris-evokernel/src/core.rs`
- Modify: `crates/oris-evokernel/tests/evolution_lifecycle_regression.rs`
- Test: `crates/oris-evokernel/tests/evolution_lifecycle_regression.rs`

**Step 1: Write the failing test**

Add a regression where an imported remote asset fails replay validation enough to trigger revocation, then assert the matching `PromotionEvaluated` event contains a stable evidence summary fragment such as `source_sender_id=node-remote` and `phase=replay_failure_revocation`.

**Step 2: Run test to verify it fails**

Run: `cargo test -p oris-evokernel --test evolution_lifecycle_regression remote_replay_failure_ -- --nocapture`
Expected: FAIL because the current evidence summary does not consistently include remote attribution.

**Step 3: Write minimal implementation**

Refactor replay-failure revocation evidence building to use shared remote attribution lookup and stable summary formatting.

**Step 4: Run test to verify it passes**

Run: `cargo test -p oris-evokernel --test evolution_lifecycle_regression remote_replay_failure_ -- --nocapture`
Expected: PASS

**Step 5: Commit**

```bash
git add crates/oris-evokernel/src/core.rs crates/oris-evokernel/tests/evolution_lifecycle_regression.rs
git commit -m "feat(evokernel): surface remote attribution in revocation evidence"
```

### Task 3: Extend the travel-network runtime regression

**Files:**
- Modify: `crates/oris-runtime/tests/agent_self_evolution_travel_network.rs`
- Test: `crates/oris-runtime/tests/agent_self_evolution_travel_network.rs`

**Step 1: Write the failing test**

Extend the existing travel-network scenario so it asserts that the imported remote sender identity is visible in the consumer-side event/audit trail across import, replay reuse, and revoke handling.

**Step 2: Run test to verify it fails**

Run: `cargo test -p oris-runtime --test agent_self_evolution_travel_network --features full-evolution-experimental -- --nocapture`
Expected: FAIL because the runtime regression does not yet assert the new revoke/attribution hardening behavior.

**Step 3: Write minimal implementation**

Adjust runtime-facing behavior only as needed to expose the now-stable evokernel evidence or revoke rejection semantics in the test surface.

**Step 4: Run test to verify it passes**

Run: `cargo test -p oris-runtime --test agent_self_evolution_travel_network --features full-evolution-experimental -- --nocapture`
Expected: PASS

**Step 5: Commit**

```bash
git add crates/oris-runtime/tests/agent_self_evolution_travel_network.rs crates/oris-evokernel/src/core.rs
git commit -m "test(runtime): lock federated attribution hardening flow"
```

### Task 4: Run issue validation and release validation

**Files:**
- Modify: `crates/oris-evokernel/src/core.rs`
- Modify: `crates/oris-evokernel/tests/evolution_lifecycle_regression.rs`
- Modify: `crates/oris-runtime/tests/agent_self_evolution_travel_network.rs`
- Modify: `docs/plans/2026-03-14-federated-evolution-hardening-design.md`
- Modify: `docs/plans/2026-03-14-federated-evolution-hardening-implementation-plan.md`

**Step 1: Run targeted issue validation**

Run: `cargo test -p oris-evokernel --lib`
Expected: PASS

Run: `cargo test -p oris-runtime --test agent_self_evolution_travel_network --features full-evolution-experimental -- --nocapture`
Expected: PASS

**Step 2: Run formatting and workspace validation**

Run: `cargo fmt --all -- --check`
Expected: PASS

Run: `cargo test --workspace`
Expected: PASS

**Step 3: Prepare release notes and version bumps**

Update crate versions and draft the matching `RELEASE_v<version>.md` file based on the issue scope and validation evidence.

**Step 4: Run release validation**

Run the maintainer release sequence, including `cargo build --verbose --all --release --all-features`, `cargo test --release --all-features`, and the required `cargo publish` dry runs / publishes.

**Step 5: Commit**

```bash
git add crates/oris-evokernel/src/core.rs crates/oris-evokernel/tests/evolution_lifecycle_regression.rs crates/oris-runtime/tests/agent_self_evolution_travel_network.rs docs/plans/2026-03-14-federated-evolution-hardening-design.md docs/plans/2026-03-14-federated-evolution-hardening-implementation-plan.md RELEASE_v<version>.md Cargo.toml crates/oris-evokernel/Cargo.toml crates/oris-runtime/Cargo.toml
git commit -m "chore(release): prepare federated hardening release"
```
