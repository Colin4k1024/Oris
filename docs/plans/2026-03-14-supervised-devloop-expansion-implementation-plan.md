# Supervised Devloop Expansion Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Expand supervised devloop from one docs Markdown file to a small bounded docs multi-file workflow while keeping failure behavior fail-closed and reason-code consistent across API responses and evolution events.

**Architecture:** Add one new bounded task class, `DocsMultiFile`, and centralize supervised-devloop request boundary validation inside `crates/oris-evokernel/src/core.rs`. Keep the existing byte, line, timeout, and validation-budget gates unchanged, and route every failure through the existing normalized mutation-needed failure contract so response/event consistency is preserved.

**Tech Stack:** Rust, `tokio`, cargo test/fmt, GitHub issue workflow.

Execution discipline: `@test-driven-development`, `@verification-before-completion`.

---

### Task 1: Capture the Expanded Success Path with a Failing Evokernel Test

**Files:**
- Modify: `crates/oris-evokernel/tests/evolution_lifecycle_regression.rs`
- Test: `crates/oris-evokernel/tests/evolution_lifecycle_regression.rs`

**Step 1: Write the failing test**

Add a regression for an approved request that touches two docs Markdown files
and should execute successfully under the new bounded task class.

```rust
#[tokio::test]
async fn supervised_devloop_executes_bounded_multifile_docs_task_after_approval() {
    let (_workspace, _store, evo) = test_evo("supervised-devloop-multifile-approved");
    let request = devloop_request_with_files(
        "task-docs-multifile-approved",
        vec!["docs/a.md", "docs/b.md"],
        true,
    );

    let outcome = evo
        .run_supervised_devloop(&"run-supervised-devloop-multifile-approved".to_string(), &request, proposal_diff_for_files(&["docs/a.md", "docs/b.md"]), None)
        .await
        .unwrap();

    assert_eq!(outcome.status, SupervisedDevloopStatus::Executed);
    assert_eq!(outcome.task_class, Some(BoundedTaskClass::DocsMultiFile));
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p oris-evokernel --test evolution_lifecycle_regression supervised_devloop_executes_bounded_multifile_docs_task_after_approval -- --nocapture`
Expected: FAIL because the current classifier only accepts a single docs file.

**Step 3: Commit**

Do not commit yet. Keep moving through the red-green cycle.

### Task 2: Add Failure Regressions for New Multi-File Bounds

**Files:**
- Modify: `crates/oris-evokernel/tests/evolution_lifecycle_regression.rs`
- Test: `crates/oris-evokernel/tests/evolution_lifecycle_regression.rs`

**Step 1: Write the failing tests**

Add focused regressions for:
- a mixed-scope request such as `["docs/a.md", "src/lib.rs"]`
- an over-limit request such as four docs Markdown files

Each test should assert:
- `RejectedByPolicy`
- `failure_contract.reason_code == PolicyDenied`
- the emitted `EvolutionEvent::MutationRejected.reason_code` is
  `"policy_denied"`

```rust
#[tokio::test]
async fn supervised_devloop_rejects_multifile_docs_request_with_out_of_scope_path() { /* ... */ }

#[tokio::test]
async fn supervised_devloop_rejects_multifile_docs_request_over_file_limit() { /* ... */ }
```

**Step 2: Run tests to verify they fail**

Run: `cargo test -p oris-evokernel --test evolution_lifecycle_regression supervised_devloop_rejects_multifile_docs_request_ -- --nocapture`
Expected: FAIL until the classifier and bounded-scope helper understand the new rules.

### Task 3: Implement Minimal Bounded Multi-File Classification

**Files:**
- Modify: `crates/oris-agent-contract/src/lib.rs`
- Modify: `crates/oris-evokernel/src/core.rs`
- Test: `crates/oris-evokernel/tests/evolution_lifecycle_regression.rs`

**Step 1: Write minimal implementation**

Add `DocsMultiFile` to the public contract and replace the current
first-file-only classifier with a helper that validates the entire declared file
set.

```rust
pub enum BoundedTaskClass {
    DocsSingleFile,
    DocsMultiFile,
}

const SUPERVISED_DEVLOOP_MAX_DOC_FILES: usize = 3;

fn classify_supervised_devloop_request(
    request: &SupervisedDevloopRequest,
) -> Option<BoundedTaskClass> {
    let files = normalize_supervised_devloop_files(&request.proposal.files)?;
    match files.len() {
        1 => Some(BoundedTaskClass::DocsSingleFile),
        2..=SUPERVISED_DEVLOOP_MAX_DOC_FILES => Some(BoundedTaskClass::DocsMultiFile),
        _ => None,
    }
}
```

**Step 2: Run the focused tests to verify they pass**

Run: `cargo test -p oris-evokernel --test evolution_lifecycle_regression supervised_devloop_executes_bounded_multifile_docs_task_after_approval -- --nocapture`
Expected: PASS.

Run: `cargo test -p oris-evokernel --test evolution_lifecycle_regression supervised_devloop_rejects_multifile_docs_request_ -- --nocapture`
Expected: PASS.

**Step 3: Refactor**

Keep helper naming explicit and reuse it only for supervised-devloop boundary
checks. Do not widen the mutation-needed policy surface beyond docs Markdown
files.

### Task 4: Lock Runtime Facade Exposure

**Files:**
- Modify: `crates/oris-runtime/tests/evolution_feature_wiring.rs`
- Test: `crates/oris-runtime/tests/evolution_feature_wiring.rs`

**Step 1: Write the failing assertion**

Add a lightweight compile/runtime assertion that references the new task class.

```rust
let task_class = oris_runtime::agent_contract::BoundedTaskClass::DocsMultiFile;
assert!(matches!(
    task_class,
    oris_runtime::agent_contract::BoundedTaskClass::DocsMultiFile
));
```

**Step 2: Run test to verify it passes**

Run: `cargo test -p oris-runtime --test evolution_feature_wiring --features full-evolution-experimental -- --nocapture`
Expected: PASS once the public contract export is updated.

### Task 5: Run the Issue Validation Floor

**Files:**
- Modify: none
- Test: workspace validation commands

**Step 1: Run formatter check**

Run: `cargo fmt --all -- --check`
Expected: PASS.

**Step 2: Run issue-specific regressions**

Run: `cargo test -p oris-evokernel --test evolution_lifecycle_regression supervised_devloop_ -- --nocapture`
Expected: PASS.

Run: `cargo test -p oris-runtime --test evolution_feature_wiring --features full-evolution-experimental -- --nocapture`
Expected: PASS.

**Step 3: Run feature-class release validation**

Run: `cargo test --workspace`
Expected: PASS.

Run: `cargo build --verbose --all --release --all-features`
Expected: PASS.

Run: `cargo test --release --all-features`
Expected: PASS.

**Step 4: Commit**

```bash
git add crates/oris-agent-contract/src/lib.rs crates/oris-evokernel/src/core.rs crates/oris-evokernel/tests/evolution_lifecycle_regression.rs crates/oris-runtime/tests/evolution_feature_wiring.rs docs/plans/2026-03-14-supervised-devloop-expansion-design.md docs/plans/2026-03-14-supervised-devloop-expansion-implementation-plan.md
git commit -m "feat(evokernel): expand supervised devloop docs scope"
```

### Task 6: Release Workflow

**Files:**
- Modify: `crates/oris-runtime/Cargo.toml`
- Modify: `RELEASE_v<version>.md`

**Step 1: Prepare release artifacts**

Choose the version bump from the maintainer policy. This issue is a `feature`,
so default to a `minor` release unless compatibility review requires a higher
decision.

**Step 2: Run publish commands**

Run: `cargo publish -p oris-runtime --all-features --dry-run`
Expected: PASS.

Run: `cargo publish -p oris-runtime --all-features`
Expected: PASS.

**Step 3: Finalize**

Push branch and tag, update issue status to released, then close the issue with
the shipped version and validation evidence.
