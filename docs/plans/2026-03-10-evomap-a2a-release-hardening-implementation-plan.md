# EvoMap A2A Release Hardening Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Deliver a 2-week release-control hardening slice for EvoMap/A2A by enforcing deterministic gate evidence, closing SQLite/Postgres semantic persistence drift in runtime repositories, and making release-candidate checks reproducible in CI.

**Architecture:** Keep `oris-runtime` as semantic source of truth, harden `oris-orchestrator` gate evidence so PR readiness is fail-closed, and close `RuntimeRepository` parity in `oris-execution-runtime` Postgres backend so semantic write paths behave consistently across SQLite/Postgres. Add one scripted release-gate entrypoint and wire it into CI for repeatable evidence output.

**Tech Stack:** Rust (`axum`, `tokio`, `sqlx`, `rusqlite`), Bash, GitHub Actions, cargo test/fmt/clippy.

Execution discipline: `@test-driven-development`, `@verification-before-completion`.

---

### Task 1: Harden Orchestrator Evidence Gate Schema

**Files:**
- Modify: `crates/oris-orchestrator/src/evidence.rs`
- Modify: `crates/oris-orchestrator/tests/evidence_gate.rs`
- Test: `crates/oris-orchestrator/tests/evidence_gate.rs`

**Step 1: Write the failing test**

Add tests that require all explicit gate dimensions:

```rust
#[test]
fn pr_ready_requires_backend_parity_and_contract_e2e_green() {
    let bundle = EvidenceBundle {
        run_id: "run-1".into(),
        build_ok: true,
        contract_ok: true,
        e2e_ok: true,
        backend_parity_ok: false,
        policy_ok: true,
    };
    assert!(!ValidationGate::is_pr_ready(&bundle));
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p oris-orchestrator pr_ready_requires_backend_parity_and_contract_e2e_green -- --nocapture`  
Expected: FAIL because `EvidenceBundle` has no `contract_ok/e2e_ok/backend_parity_ok`.

**Step 3: Write minimal implementation**

Extend `EvidenceBundle` and gate logic:

```rust
pub struct EvidenceBundle {
    pub run_id: String,
    pub build_ok: bool,
    pub contract_ok: bool,
    pub e2e_ok: bool,
    pub backend_parity_ok: bool,
    pub policy_ok: bool,
}

impl ValidationGate {
    pub fn is_pr_ready(bundle: &EvidenceBundle) -> bool {
        bundle.build_ok
            && bundle.contract_ok
            && bundle.e2e_ok
            && bundle.backend_parity_ok
            && bundle.policy_ok
    }
}
```

**Step 4: Run tests to verify pass**

Run: `cargo test -p oris-orchestrator evidence_gate -- --nocapture`  
Expected: PASS.

**Step 5: Commit**

```bash
git add crates/oris-orchestrator/src/evidence.rs crates/oris-orchestrator/tests/evidence_gate.rs
git commit -m "feat(orchestrator): require contract e2e and backend parity in evidence gate"
```

### Task 2: Make Coordinator Gate Failure Deterministic and Explainable

**Files:**
- Modify: `crates/oris-orchestrator/src/coordinator.rs`
- Modify: `crates/oris-orchestrator/tests/coordinator_flow.rs`
- Test: `crates/oris-orchestrator/tests/coordinator_flow.rs`

**Step 1: Write the failing test**

Add a test that forces non-green evidence and verifies fail-closed behavior:

```rust
#[tokio::test]
async fn run_task_blocks_when_backend_parity_gate_is_false() {
    // construct coordinator and inject non-green validation summary
    // expect CoordinatorError kind == "validation"
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p oris-orchestrator run_task_blocks_when_backend_parity_gate_is_false -- --nocapture`  
Expected: FAIL because coordinator always builds all-green evidence on success path.

**Step 3: Write minimal implementation**

Introduce a small validation summary input and enforce it:

```rust
#[derive(Debug, Clone)]
pub struct ValidationSummary {
    pub build_ok: bool,
    pub contract_ok: bool,
    pub e2e_ok: bool,
    pub backend_parity_ok: bool,
    pub policy_ok: bool,
}

let evidence = EvidenceBundle::new(&start_ack.session_id, summary);
if !ValidationGate::is_pr_ready(&evidence) {
    return Err(CoordinatorError::validation("validation gate denied PR readiness"));
}
```

Keep `run_task(...)` backward-compatible by delegating to `run_task_with_validation(...)` with default all-true summary.

**Step 4: Run tests to verify pass**

Run: `cargo test -p oris-orchestrator coordinator_flow -- --nocapture`  
Expected: PASS, including existing `ReleasePendingApproval` path and new gate-denied path.

**Step 5: Commit**

```bash
git add crates/oris-orchestrator/src/coordinator.rs crates/oris-orchestrator/tests/coordinator_flow.rs
git commit -m "feat(orchestrator): enforce deterministic fail-closed validation summary in coordinator"
```

### Task 3: Implement Postgres RuntimeRepository Parity (Bounty/Swarm/Worker)

**Files:**
- Modify: `crates/oris-execution-runtime/src/postgres_runtime_repository.rs`
- Test: `crates/oris-execution-runtime/src/postgres_runtime_repository.rs` (tests module)

**Step 1: Write the failing tests**

Add trait-level parity tests that use `RuntimeRepository` APIs (not helper methods):

```rust
#[test]
fn runtime_repository_bounty_roundtrip_contract_postgres_when_env_is_set() {
    // repo.upsert_bounty -> get_bounty -> accept_bounty -> close_bounty
    // assert state transitions match sqlite contract expectations
}
```

```rust
#[test]
fn runtime_repository_worker_roundtrip_contract_postgres_when_env_is_set() {
    // repo.register_worker -> get_worker -> list_workers -> heartbeat_worker
}
```

**Step 2: Run tests to verify they fail**

Run: `ORIS_TEST_POSTGRES_URL=postgres://postgres:postgres@127.0.0.1:5432/oris_test cargo test -p oris-execution-runtime --features "sqlite-persistence,kernel-postgres" runtime_repository_bounty_roundtrip_contract_postgres_when_env_is_set runtime_repository_worker_roundtrip_contract_postgres_when_env_is_set -- --nocapture --test-threads=1`  
Expected: FAIL because trait methods are TODO no-ops in Postgres impl.

**Step 3: Write minimal implementation**

Implement trait methods by mapping `*Record` <-> existing Postgres row helpers:

```rust
fn upsert_bounty(&self, bounty: &BountyRecord) -> Result<(), KernelError> {
    self.create_bounty(
        &bounty.bounty_id,
        &bounty.title,
        bounty.description.as_deref(),
        bounty.reward,
        &bounty.created_by,
        ms_to_dt(bounty.created_at_ms),
    )?;
    if bounty.status == BountyStatus::Accepted {
        self.accept_bounty(
            &bounty.bounty_id,
            bounty.accepted_by.as_deref().unwrap_or("unknown"),
            ms_to_dt(bounty.accepted_at_ms.unwrap_or(bounty.created_at_ms)),
        )?;
    }
    if bounty.status == BountyStatus::Closed {
        self.close_bounty(
            &bounty.bounty_id,
            ms_to_dt(bounty.closed_at_ms.unwrap_or(bounty.created_at_ms)),
        )?;
    }
    Ok(())
}
```

Apply the same pattern for `get_bounty/list_bounties/accept_bounty/close_bounty`, `upsert_swarm_decomposition/get_swarm_decomposition`, and worker methods.

**Step 4: Run tests to verify pass**

Run: `ORIS_TEST_POSTGRES_URL=postgres://postgres:postgres@127.0.0.1:5432/oris_test cargo test -p oris-execution-runtime --features "sqlite-persistence,kernel-postgres" runtime_repository_bounty_roundtrip_contract_postgres_when_env_is_set runtime_repository_worker_roundtrip_contract_postgres_when_env_is_set postgres_swarm_task_roundtrip_when_env_is_set -- --nocapture --test-threads=1`  
Expected: PASS.

**Step 5: Commit**

```bash
git add crates/oris-execution-runtime/src/postgres_runtime_repository.rs
git commit -m "feat(execution-runtime): implement postgres runtime repository bounty swarm worker parity"
```

### Task 4: Implement Postgres RuntimeRepository Parity (Recipe/Organism/Session/Dispute)

**Files:**
- Modify: `crates/oris-execution-runtime/src/postgres_runtime_repository.rs`
- Test: `crates/oris-execution-runtime/src/postgres_runtime_repository.rs` (tests module)

**Step 1: Write the failing tests**

Add trait-level tests for remaining semantic categories:

```rust
#[test]
fn runtime_repository_recipe_organism_roundtrip_contract_postgres_when_env_is_set() {
    // create_recipe/get_recipe/fork_recipe/list_recipes + express/get/update organism
}
```

```rust
#[test]
fn runtime_repository_session_dispute_roundtrip_contract_postgres_when_env_is_set() {
    // create_session/add_message/get_history + open/get/list/resolve dispute
}
```

**Step 2: Run tests to verify they fail**

Run: `ORIS_TEST_POSTGRES_URL=postgres://postgres:postgres@127.0.0.1:5432/oris_test cargo test -p oris-execution-runtime --features "sqlite-persistence,kernel-postgres" runtime_repository_recipe_organism_roundtrip_contract_postgres_when_env_is_set runtime_repository_session_dispute_roundtrip_contract_postgres_when_env_is_set -- --nocapture --test-threads=1`  
Expected: FAIL because Postgres trait methods still return empty/no-op.

**Step 3: Write minimal implementation**

Implement trait methods by delegating to existing Postgres helper methods:

```rust
fn create_recipe(&self, recipe: &RecipeRecord) -> Result<(), KernelError> {
    self.create_recipe(&PostgresRecipeRow {
        recipe_id: recipe.recipe_id.clone(),
        name: recipe.name.clone(),
        description: recipe.description.clone(),
        gene_sequence_json: recipe.gene_sequence_json.clone(),
        author_id: recipe.author_id.clone(),
        forked_from: recipe.forked_from.clone(),
        created_at: ms_to_dt(recipe.created_at_ms),
        updated_at: ms_to_dt(recipe.updated_at_ms),
        is_public: recipe.is_public,
    })
}
```

Also implement `fork_recipe/list_recipes/express_organism/get_organism/update_organism/create_session/get_session/add_session_message/get_session_history/open_dispute/get_dispute/get_disputes_for_bounty/resolve_dispute`.

**Step 4: Run tests to verify pass**

Run: `ORIS_TEST_POSTGRES_URL=postgres://postgres:postgres@127.0.0.1:5432/oris_test cargo test -p oris-execution-runtime --features "sqlite-persistence,kernel-postgres" runtime_repository_recipe_organism_roundtrip_contract_postgres_when_env_is_set runtime_repository_session_dispute_roundtrip_contract_postgres_when_env_is_set postgres_dispute_lifecycle_roundtrip_when_env_is_set -- --nocapture --test-threads=1`  
Expected: PASS.

**Step 5: Commit**

```bash
git add crates/oris-execution-runtime/src/postgres_runtime_repository.rs
git commit -m "feat(execution-runtime): implement postgres runtime repository recipe organism session dispute parity"
```

### Task 5: Add Cross-Backend Semantic Parity Contract Tests

**Files:**
- Modify: `crates/oris-execution-runtime/src/postgres_runtime_repository.rs` (tests module)
- Modify: `crates/oris-execution-runtime/src/sqlite_runtime_repository.rs` (tests module, only if shared helper is needed)
- Test: `crates/oris-execution-runtime/src/postgres_runtime_repository.rs`

**Step 1: Write the failing test harness**

Create a reusable trait test harness run against both backends:

```rust
trait SemanticContractHarness: RuntimeRepository {
    fn seed_semantic_records(&self, prefix: &str);
    fn assert_semantic_roundtrip(&self, prefix: &str);
}
```

Add:

```rust
#[test]
fn runtime_repository_semantic_contract_sqlite() { /* ... */ }

#[test]
fn runtime_repository_semantic_contract_postgres_when_env_is_set() { /* ... */ }
```

**Step 2: Run tests to verify they fail**

Run: `ORIS_TEST_POSTGRES_URL=postgres://postgres:postgres@127.0.0.1:5432/oris_test cargo test -p oris-execution-runtime --features "sqlite-persistence,kernel-postgres" runtime_repository_semantic_contract_ -- --nocapture --test-threads=1`  
Expected: FAIL until all trait parity methods are implemented.

**Step 3: Write minimal implementation**

Complete helper methods and assertions so both backends run the same contract checks:

```rust
fn assert_semantic_roundtrip<R: RuntimeRepository>(repo: &R, prefix: &str) {
    // bounty -> worker -> recipe -> organism -> session -> dispute
    // assert deterministic state transitions and non-empty retrievals
}
```

**Step 4: Run tests to verify pass**

Run: `ORIS_TEST_POSTGRES_URL=postgres://postgres:postgres@127.0.0.1:5432/oris_test cargo test -p oris-execution-runtime --features "sqlite-persistence,kernel-postgres" runtime_repository_semantic_contract_ -- --nocapture --test-threads=1`  
Expected: PASS.

**Step 5: Commit**

```bash
git add crates/oris-execution-runtime/src/postgres_runtime_repository.rs crates/oris-execution-runtime/src/sqlite_runtime_repository.rs
git commit -m "test(execution-runtime): add sqlite postgres semantic contract parity harness"
```

### Task 6: Add Deterministic EvoMap Release Gate Script and CI Wiring

**Files:**
- Create: `scripts/run_evomap_release_gate.sh`
- Modify: `.github/workflows/ci.yml`
- Modify: `scripts/run_orchestrator_checks.sh` (optional delegation)
- Test: CI command paths in local dry run

**Step 1: Write the failing CI expectation**

Add CI step first (red state expected until script exists):

```yaml
- name: Run EvoMap release hardening gate
  shell: bash
  run: |
    bash scripts/run_evomap_release_gate.sh
```

**Step 2: Run script command to verify missing/failing state**

Run: `bash scripts/run_evomap_release_gate.sh`  
Expected: FAIL because script is not present yet.

**Step 3: Write minimal implementation**

Create script that runs fixed gate sequence and emits evidence summary:

```bash
#!/usr/bin/env bash
set -euo pipefail

mkdir -p target
cargo test -p oris-orchestrator evidence_gate coordinator_flow -- --nocapture
cargo test -p oris-runtime --features "full-evolution-experimental execution-server sqlite-persistence" \
  execution_server::api_handlers::tests::evomap_semantic_contract_e2e_covers_protocol_task_asset_and_governance_flows \
  execution_server::api_handlers::tests::audit_logs_capture_semantic_protocol_core_actions \
  -- --nocapture --test-threads=1
cargo test -p oris-execution-runtime --features "sqlite-persistence,kernel-postgres" \
  runtime_repository_semantic_contract_ -- --nocapture --test-threads=1
printf '{"gate":"evomap-release-hardening","status":"pass"}\n' > target/evomap-release-evidence.json
```

**Step 4: Run script to verify pass**

Run: `ORIS_TEST_POSTGRES_URL=postgres://postgres:postgres@127.0.0.1:5432/oris_test bash scripts/run_evomap_release_gate.sh`  
Expected: PASS and `target/evomap-release-evidence.json` created.

**Step 5: Commit**

```bash
git add scripts/run_evomap_release_gate.sh .github/workflows/ci.yml scripts/run_orchestrator_checks.sh
git commit -m "ci: add deterministic evomap release hardening gate"
```

### Task 7: Document Release-Gate Evidence and Operator Checklist

**Files:**
- Modify: `docs/production-operations-guide.md`
- Modify: `docs/evokernel/open-issue-priority-plan.md`
- Test: markdown lint/readability pass via docs sanity grep

**Step 1: Write failing doc checks**

Add simple doc assertions to ensure checklist terms exist:

```bash
rg -n "EvoMap release gate|backend parity|evidence bundle id" docs/production-operations-guide.md docs/evokernel/open-issue-priority-plan.md
```

**Step 2: Run check to verify it fails**

Run: `rg -n "EvoMap release gate|backend parity|evidence bundle id" docs/production-operations-guide.md docs/evokernel/open-issue-priority-plan.md`  
Expected: FAIL or partial misses before updates.

**Step 3: Write minimal documentation updates**

Add a concise operator checklist:

```md
### EvoMap release gate (required before publish)
- contract gate: pass
- e2e gate: pass
- backend parity gate (sqlite/postgres): pass or approved exception
- evidence bundle id attached to approval record
```

Update queue rules to reference `scripts/run_evomap_release_gate.sh` as required pre-publish evidence command.

**Step 4: Re-run doc checks**

Run: `rg -n "EvoMap release gate|backend parity|evidence bundle id|run_evomap_release_gate.sh" docs/production-operations-guide.md docs/evokernel/open-issue-priority-plan.md`  
Expected: PASS.

**Step 5: Commit**

```bash
git add docs/production-operations-guide.md docs/evokernel/open-issue-priority-plan.md
git commit -m "docs: add evomap release gate evidence checklist"
```

### Final Verification Before Merge

Run:

```bash
cargo fmt --all -- --check
cargo clippy -p oris-orchestrator -p oris-execution-runtime --all-targets --all-features -- -D warnings
cargo test -p oris-orchestrator
ORIS_TEST_POSTGRES_URL=postgres://postgres:postgres@127.0.0.1:5432/oris_test \
  cargo test -p oris-execution-runtime --features "sqlite-persistence,kernel-postgres" -- --nocapture --test-threads=1
ORIS_TEST_POSTGRES_URL=postgres://postgres:postgres@127.0.0.1:5432/oris_test \
  bash scripts/run_evomap_release_gate.sh
```

Expected: all PASS, deterministic evidence file generated, no diff after rerun except timestamped artifacts in `target/`.
