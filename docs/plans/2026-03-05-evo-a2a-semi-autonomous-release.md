# Evo A2A Semi-Autonomous Release Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Build an external orchestrator that automates issue-to-merge and keeps exactly one human approval gate before release publish.

**Architecture:** Add a new orchestrator crate that drives GitHub intake, A2A runtime sessions, validation, PR automation, and release gating. Keep `oris-runtime` as the policy and execution source of truth, and persist deterministic evidence for each transition. Enforce state-machine transitions with typed domain models and integration tests.

**Tech Stack:** Rust workspace crates, Tokio async runtime, `serde` for contracts, `reqwest` for GitHub and runtime API calls, existing `oris-agent-contract`/`oris-runtime` A2A routes, GitHub CLI for local rehearsal.

---

Skills referenced:

- @superpowers/test-driven-development
- @superpowers/verification-before-completion
- @superpowers/requesting-code-review

### Task 1: Scaffold Orchestrator Crate and State Types

**Files:**

- Create: `crates/oris-orchestrator/Cargo.toml`
- Create: `crates/oris-orchestrator/src/lib.rs`
- Create: `crates/oris-orchestrator/src/state.rs`
- Create: `crates/oris-orchestrator/tests/state_machine.rs`
- Modify: `Cargo.toml`

**Step 1: Write the failing test**

```rust
use oris_orchestrator::state::{TaskState, TaskTransitionError, transition};

#[test]
fn release_requires_explicit_approval_path() {
    let state = transition(TaskState::Merged, "request_release").unwrap();
    assert_eq!(state, TaskState::ReleasePendingApproval);

    let err = transition(TaskState::Merged, "publish_without_approval").unwrap_err();
    assert_eq!(err, TaskTransitionError::InvalidTransition);
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p oris-orchestrator --test state_machine release_requires_explicit_approval_path`
Expected: FAIL because crate or transition types do not exist.

**Step 3: Write minimal implementation**

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskState {
    Queued,
    Planned,
    Dispatched,
    InProgress,
    Validated,
    PRReady,
    Merged,
    ReleasePendingApproval,
    Released,
    FailedRetryable,
    FailedTerminal,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskTransitionError {
    InvalidTransition,
}

pub fn transition(state: TaskState, event: &str) -> Result<TaskState, TaskTransitionError> {
    match (state, event) {
        (TaskState::Merged, "request_release") => Ok(TaskState::ReleasePendingApproval),
        (TaskState::ReleasePendingApproval, "approve_release") => Ok(TaskState::Released),
        _ => Err(TaskTransitionError::InvalidTransition),
    }
}
```

**Step 4: Run test to verify it passes**

Run: `cargo test -p oris-orchestrator --test state_machine release_requires_explicit_approval_path`
Expected: PASS.

**Step 5: Commit**

```bash
git add Cargo.toml crates/oris-orchestrator/Cargo.toml crates/oris-orchestrator/src/lib.rs crates/oris-orchestrator/src/state.rs crates/oris-orchestrator/tests/state_machine.rs
git commit -m "feat(orchestrator): add task state machine scaffold"
```

### Task 2: Add TaskSpec Intake Model and Validation

**Files:**

- Create: `crates/oris-orchestrator/src/task_spec.rs`
- Create: `crates/oris-orchestrator/tests/task_spec_validation.rs`
- Modify: `crates/oris-orchestrator/src/lib.rs`

**Step 1: Write the failing test**

```rust
use oris_orchestrator::task_spec::TaskSpec;

#[test]
fn task_spec_rejects_empty_allowed_paths() {
    let spec = TaskSpec::new("issue-123", "Fix build", vec![]);
    assert!(spec.is_err());
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p oris-orchestrator --test task_spec_validation task_spec_rejects_empty_allowed_paths`
Expected: FAIL because `TaskSpec` is not implemented.

**Step 3: Write minimal implementation**

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskSpec {
    pub issue_id: String,
    pub title: String,
    pub allowed_paths: Vec<String>,
}

impl TaskSpec {
    pub fn new(issue_id: &str, title: &str, allowed_paths: Vec<String>) -> Result<Self, &'static str> {
        if allowed_paths.is_empty() {
            return Err("allowed_paths must not be empty");
        }
        Ok(Self {
            issue_id: issue_id.to_string(),
            title: title.to_string(),
            allowed_paths,
        })
    }
}
```

**Step 4: Run test to verify it passes**

Run: `cargo test -p oris-orchestrator --test task_spec_validation`
Expected: PASS.

**Step 5: Commit**

```bash
git add crates/oris-orchestrator/src/lib.rs crates/oris-orchestrator/src/task_spec.rs crates/oris-orchestrator/tests/task_spec_validation.rs
git commit -m "feat(orchestrator): add task spec validation"
```

### Task 3: Implement Runtime A2A Client for Handshake and Session Flow

**Files:**

- Create: `crates/oris-orchestrator/src/runtime_client.rs`
- Create: `crates/oris-orchestrator/tests/runtime_client_contract.rs`
- Modify: `crates/oris-orchestrator/src/lib.rs`

**Step 1: Write the failing test**

```rust
use oris_orchestrator::runtime_client::A2aSessionRequest;

#[test]
fn start_session_rejects_invalid_protocol_version() {
    let req = A2aSessionRequest::start("sender-a", "0.0.1", "task-1", "summary");
    assert!(req.validate().is_err());
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p oris-orchestrator --test runtime_client_contract start_session_rejects_invalid_protocol_version`
Expected: FAIL because request type does not exist.

**Step 3: Write minimal implementation**

```rust
pub const EXPECTED_PROTOCOL_VERSION: &str = "0.1.0-experimental";

pub struct A2aSessionRequest {
    pub sender_id: String,
    pub protocol_version: String,
    pub task_id: String,
    pub task_summary: String,
}

impl A2aSessionRequest {
    pub fn start(sender_id: &str, protocol_version: &str, task_id: &str, task_summary: &str) -> Self {
        Self {
            sender_id: sender_id.to_string(),
            protocol_version: protocol_version.to_string(),
            task_id: task_id.to_string(),
            task_summary: task_summary.to_string(),
        }
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        if self.protocol_version != EXPECTED_PROTOCOL_VERSION {
            return Err("incompatible a2a task session protocol version");
        }
        Ok(())
    }
}
```

**Step 4: Run test to verify it passes**

Run: `cargo test -p oris-orchestrator --test runtime_client_contract`
Expected: PASS.

**Step 5: Commit**

```bash
git add crates/oris-orchestrator/src/lib.rs crates/oris-orchestrator/src/runtime_client.rs crates/oris-orchestrator/tests/runtime_client_contract.rs
git commit -m "feat(orchestrator): add runtime A2A request contract"
```

### Task 4: Add Validation Gate and Replay Evidence Bundle

**Files:**

- Create: `crates/oris-orchestrator/src/evidence.rs`
- Create: `crates/oris-orchestrator/tests/evidence_gate.rs`
- Modify: `crates/oris-orchestrator/src/lib.rs`

**Step 1: Write the failing test**

```rust
use oris_orchestrator::evidence::{EvidenceBundle, ValidationGate};

#[test]
fn pr_ready_requires_full_green_validation() {
    let bundle = EvidenceBundle::new("run-1", false, false, false);
    assert_eq!(ValidationGate::is_pr_ready(&bundle), false);
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p oris-orchestrator --test evidence_gate pr_ready_requires_full_green_validation`
Expected: FAIL because evidence types are missing.

**Step 3: Write minimal implementation**

```rust
pub struct EvidenceBundle {
    pub run_id: String,
    pub build_ok: bool,
    pub tests_ok: bool,
    pub policy_ok: bool,
}

impl EvidenceBundle {
    pub fn new(run_id: &str, build_ok: bool, tests_ok: bool, policy_ok: bool) -> Self {
        Self {
            run_id: run_id.to_string(),
            build_ok,
            tests_ok,
            policy_ok,
        }
    }
}

pub struct ValidationGate;

impl ValidationGate {
    pub fn is_pr_ready(bundle: &EvidenceBundle) -> bool {
        bundle.build_ok && bundle.tests_ok && bundle.policy_ok
    }
}
```

**Step 4: Run test to verify it passes**

Run: `cargo test -p oris-orchestrator --test evidence_gate`
Expected: PASS.

**Step 5: Commit**

```bash
git add crates/oris-orchestrator/src/lib.rs crates/oris-orchestrator/src/evidence.rs crates/oris-orchestrator/tests/evidence_gate.rs
git commit -m "feat(orchestrator): add validation gate evidence model"
```

### Task 5: Implement GitHub Adapter for Branch and PR Automation

**Files:**

- Create: `crates/oris-orchestrator/src/github_adapter.rs`
- Create: `crates/oris-orchestrator/tests/github_adapter_payload.rs`
- Modify: `crates/oris-orchestrator/src/lib.rs`

**Step 1: Write the failing test**

```rust
use oris_orchestrator::github_adapter::PrPayload;

#[test]
fn pr_payload_requires_evidence_bundle_reference() {
    let payload = PrPayload::new("issue-123", "codex/issue-123", "main", "", "body");
    assert!(payload.validate().is_err());
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p oris-orchestrator --test github_adapter_payload pr_payload_requires_evidence_bundle_reference`
Expected: FAIL because payload contract is missing.

**Step 3: Write minimal implementation**

```rust
pub struct PrPayload {
    pub issue_id: String,
    pub head: String,
    pub base: String,
    pub evidence_bundle_id: String,
    pub body: String,
}

impl PrPayload {
    pub fn new(issue_id: &str, head: &str, base: &str, evidence_bundle_id: &str, body: &str) -> Self {
        Self {
            issue_id: issue_id.to_string(),
            head: head.to_string(),
            base: base.to_string(),
            evidence_bundle_id: evidence_bundle_id.to_string(),
            body: body.to_string(),
        }
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        if self.evidence_bundle_id.trim().is_empty() {
            return Err("evidence_bundle_id is required");
        }
        Ok(())
    }
}
```

**Step 4: Run test to verify it passes**

Run: `cargo test -p oris-orchestrator --test github_adapter_payload`
Expected: PASS.

**Step 5: Commit**

```bash
git add crates/oris-orchestrator/src/lib.rs crates/oris-orchestrator/src/github_adapter.rs crates/oris-orchestrator/tests/github_adapter_payload.rs
git commit -m "feat(orchestrator): add github PR payload contract"
```

### Task 6: Add Release Gate with Single Human Approval

**Files:**

- Create: `crates/oris-orchestrator/src/release_gate.rs`
- Create: `crates/oris-orchestrator/tests/release_gate.rs`
- Modify: `crates/oris-orchestrator/src/lib.rs`

**Step 1: Write the failing test**

```rust
use oris_orchestrator::release_gate::{ReleaseDecision, ReleaseGate};

#[test]
fn publish_requires_explicit_approval() {
    let denied = ReleaseGate::can_publish(ReleaseDecision::Rejected);
    let approved = ReleaseGate::can_publish(ReleaseDecision::Approved);
    assert!(!denied);
    assert!(approved);
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p oris-orchestrator --test release_gate publish_requires_explicit_approval`
Expected: FAIL because release gate does not exist.

**Step 3: Write minimal implementation**

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReleaseDecision {
    Approved,
    Rejected,
}

pub struct ReleaseGate;

impl ReleaseGate {
    pub fn can_publish(decision: ReleaseDecision) -> bool {
        matches!(decision, ReleaseDecision::Approved)
    }
}
```

**Step 4: Run test to verify it passes**

Run: `cargo test -p oris-orchestrator --test release_gate`
Expected: PASS.

**Step 5: Commit**

```bash
git add crates/oris-orchestrator/src/lib.rs crates/oris-orchestrator/src/release_gate.rs crates/oris-orchestrator/tests/release_gate.rs
git commit -m "feat(orchestrator): enforce single human release gate"
```

### Task 7: Integrate End-to-End Coordinator Pipeline

**Files:**

- Create: `crates/oris-orchestrator/src/coordinator.rs`
- Create: `crates/oris-orchestrator/tests/coordinator_flow.rs`
- Modify: `crates/oris-orchestrator/src/lib.rs`

**Step 1: Write the failing test**

```rust
use oris_orchestrator::coordinator::Coordinator;

#[tokio::test]
async fn flow_reaches_release_pending_approval_before_publish() {
    let coordinator = Coordinator::for_test();
    let state = coordinator.run_single_issue("issue-123").await.unwrap();
    assert_eq!(state.as_str(), "ReleasePendingApproval");
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p oris-orchestrator --test coordinator_flow flow_reaches_release_pending_approval_before_publish`
Expected: FAIL because coordinator path is missing.

**Step 3: Write minimal implementation**

```rust
pub struct Coordinator;

impl Coordinator {
    pub fn for_test() -> Self {
        Self
    }

    pub async fn run_single_issue(&self, _issue_id: &str) -> Result<CoordinatorState, &'static str> {
        Ok(CoordinatorState::ReleasePendingApproval)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoordinatorState {
    ReleasePendingApproval,
}

impl CoordinatorState {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ReleasePendingApproval => "ReleasePendingApproval",
        }
    }
}
```

**Step 4: Run test to verify it passes**

Run: `cargo test -p oris-orchestrator --test coordinator_flow`
Expected: PASS.

**Step 5: Commit**

```bash
git add crates/oris-orchestrator/src/lib.rs crates/oris-orchestrator/src/coordinator.rs crates/oris-orchestrator/tests/coordinator_flow.rs
git commit -m "feat(orchestrator): add end-to-end coordinator skeleton"
```

### Task 8: Add CI and Local Verification Entry Points

**Files:**

- Modify: `.github/workflows/ci.yml`
- Create: `scripts/run_orchestrator_checks.sh`
- Modify: `README.md`

**Step 1: Write the failing check expectation**

Add orchestrator job command first and run it before implementation script exists.

**Step 2: Run check to verify it fails**

Run: `bash scripts/run_orchestrator_checks.sh`
Expected: FAIL because script is missing.

**Step 3: Write minimal implementation**

```bash
#!/usr/bin/env bash
set -euo pipefail

cargo fmt --all -- --check
cargo test -p oris-orchestrator
```

**Step 4: Run check to verify it passes**

Run: `bash scripts/run_orchestrator_checks.sh`
Expected: PASS.

**Step 5: Commit**

```bash
git add .github/workflows/ci.yml scripts/run_orchestrator_checks.sh README.md
git commit -m "ci: add orchestrator verification gate"
```

### Task 9: Run Final Verification and Write Release-Readiness Note

**Files:**

- Create: `docs/plans/2026-03-05-evo-a2a-semi-autonomous-release-readiness.md`

**Step 1: Run full verification**

Run: `cargo test -p oris-orchestrator && cargo test -p oris-runtime --test evolution_feature_wiring --features full-evolution-experimental`
Expected: PASS.

**Step 2: Capture evidence summary**

Document:

- passed commands
- release-gate behavior proof
- known limitations

**Step 3: Commit readiness note**

```bash
git add docs/plans/2026-03-05-evo-a2a-semi-autonomous-release-readiness.md
git commit -m "docs: add orchestrator release-readiness evidence"
```
