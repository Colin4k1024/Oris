# EvoMap A2A Compatibility Phase 1 Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add Phase 1 EvoMap compatibility by supporting `oris.a2a@1.0.0` negotiation and alias endpoints (`hello`, `tasks/distribute`, `tasks/report`, `tasks/claim` stub) without regressing existing `/v1/evolution/a2a/*` behavior.

**Architecture:** Keep Oris native session API as source of truth and add a thin compatibility layer in `api_handlers.rs` that translates EvoMap-style routes into existing session start/dispatch/progress/complete flows. Protocol compatibility is implemented as dual-version acceptance (`1.0.0` + `0.1.0-experimental`) with explicit negotiated protocol in handshake responses.

**Tech Stack:** Rust, Axum handlers, serde DTOs, Tokio tests (`cargo test -p oris-runtime --features full-evolution-experimental`).

---

### Task 1: Add protocol dual-stack negotiation primitives

**Files:**
- Modify: `crates/oris-agent-contract/src/lib.rs`
- Test: `crates/oris-agent-contract/src/lib.rs` (new `#[cfg(test)]` module)

**Step 1: Write the failing test**

```rust
#[test]
fn handshake_request_negotiates_v1_when_available() {
    let req = A2aHandshakeRequest {
        agent_id: "agent-v1".into(),
        role: AgentRole::Planner,
        capability_level: AgentCapabilityLevel::A2,
        supported_protocols: vec![
            A2aProtocol { name: A2A_PROTOCOL_NAME.into(), version: "1.0.0".into() },
            A2aProtocol { name: A2A_PROTOCOL_NAME.into(), version: "0.1.0-experimental".into() },
        ],
        advertised_capabilities: vec![A2aCapability::Coordination],
    };
    assert_eq!(req.negotiate_supported_protocol().unwrap().version, "1.0.0");
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p oris-agent-contract handshake_request_negotiates_v1_when_available -- --nocapture`  
Expected: FAIL because `negotiate_supported_protocol` does not exist.

**Step 3: Write minimal implementation**

```rust
pub const A2A_PROTOCOL_VERSION_V1: &str = "1.0.0";
pub const A2A_PROTOCOL_VERSION_EXPERIMENTAL: &str = "0.1.0-experimental";
pub const A2A_SUPPORTED_PROTOCOL_VERSIONS: [&str; 2] =
    [A2A_PROTOCOL_VERSION_V1, A2A_PROTOCOL_VERSION_EXPERIMENTAL];

impl A2aHandshakeRequest {
    pub fn negotiate_supported_protocol(&self) -> Option<A2aProtocol> {
        for version in A2A_SUPPORTED_PROTOCOL_VERSIONS {
            if self.supported_protocols.iter().any(|protocol| {
                protocol.name == A2A_PROTOCOL_NAME && protocol.version == version
            }) {
                return Some(A2aProtocol {
                    name: A2A_PROTOCOL_NAME.to_string(),
                    version: version.to_string(),
                });
            }
        }
        None
    }
}
```

**Step 4: Run test to verify it passes**

Run: `cargo test -p oris-agent-contract -- --nocapture`  
Expected: PASS.

**Step 5: Commit**

```bash
git add crates/oris-agent-contract/src/lib.rs
git commit -m "feat(agent-contract): support a2a dual-version protocol negotiation"
```

### Task 2: Update handshake/task-session protocol checks to accept dual versions

**Files:**
- Modify: `crates/oris-runtime/src/execution_server/api_handlers.rs`
- Test: `crates/oris-runtime/src/execution_server/api_handlers.rs` (existing a2a tests)

**Step 1: Write the failing test**

Add a new runtime test asserting handshake accepts only `1.0.0`:

```rust
#[tokio::test]
async fn evolution_a2a_handshake_accepts_v1_protocol_only_client() {
    // supported_protocols includes only {name: "oris.a2a", version: "1.0.0"}
    // expect accepted == true and negotiated_protocol.version == "1.0.0"
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p oris-runtime --features full-evolution-experimental evolution_a2a_handshake_accepts_v1_protocol_only_client -- --nocapture`  
Expected: FAIL because runtime currently requires only `0.1.0-experimental`.

**Step 3: Write minimal implementation**

Update handshake negotiation:

```rust
let Some(negotiated_protocol) = req.negotiate_supported_protocol() else { ... reject ... };
```

and return:

```rust
A2aHandshakeResponse {
    accepted: true,
    negotiated_protocol: Some(negotiated_protocol),
    enabled_capabilities,
    message: Some("handshake accepted".to_string()),
    error: None,
}
```

Update task session version validation to accept both `1.0.0` and `0.1.0-experimental`:

```rust
if version == "1.0.0" || version == crate::agent_contract::A2A_TASK_SESSION_PROTOCOL_VERSION {
    return Ok(());
}
```

**Step 4: Run test to verify it passes**

Run: `cargo test -p oris-runtime --features full-evolution-experimental evolution_a2a_handshake_accepts_v1_protocol_only_client evolution_a2a_remote_task_session_rejects_incompatible_protocol_version -- --nocapture`  
Expected: PASS.

**Step 5: Commit**

```bash
git add crates/oris-runtime/src/execution_server/api_handlers.rs
git commit -m "feat(runtime): accept a2a v1 and legacy task session protocols"
```

### Task 3: Add EvoMap-compatible alias endpoints and DTO translation

**Files:**
- Modify: `crates/oris-runtime/src/execution_server/api_handlers.rs`
- Test: `crates/oris-runtime/src/execution_server/api_handlers.rs` (new compatibility tests)

**Step 1: Write the failing test**

Add test for compatibility flow:

```rust
#[tokio::test]
async fn evolution_a2a_compat_distribute_and_report_map_to_session_flow() {
    // 1) POST /evolution/a2a/hello
    // 2) POST /evolution/a2a/tasks/distribute
    // 3) POST /evolution/a2a/tasks/report (running)
    // 4) POST /evolution/a2a/tasks/report (succeeded)
    // assert terminal state == Completed and replay feedback preserved
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p oris-runtime --features full-evolution-experimental evolution_a2a_compat_distribute_and_report_map_to_session_flow -- --nocapture`  
Expected: FAIL with 404 (routes not found).

**Step 3: Write minimal implementation**

Add alias routes:

```rust
.route("/evolution/a2a/hello", post(evolution_a2a_hello))
.route("/evolution/a2a/tasks/distribute", post(evolution_a2a_tasks_distribute))
.route("/evolution/a2a/tasks/claim", post(evolution_a2a_tasks_claim))
.route("/evolution/a2a/tasks/report", post(evolution_a2a_tasks_report))
```

Mapping strategy:
- `hello` -> delegates to `evolution_a2a_handshake`
- `distribute` -> delegates to `session_start` then `session_dispatch`
- `report(status=running)` -> delegates to `session_progress`
- `report(status in {succeeded, failed, cancelled})` -> delegates to `session_complete`
- `claim` -> explicit Phase 1 stub response (`bad_request: "tasks/claim not enabled in Phase 1"`), deterministic and documented in response details

**Step 4: Run test to verify it passes**

Run: `cargo test -p oris-runtime --features full-evolution-experimental evolution_a2a_compat_distribute_and_report_map_to_session_flow -- --nocapture`  
Expected: PASS.

**Step 5: Commit**

```bash
git add crates/oris-runtime/src/execution_server/api_handlers.rs
git commit -m "feat(runtime): add evomap-compatible a2a hello/distribute/report routes"
```

### Task 4: Regression and observability checks

**Files:**
- Modify: `crates/oris-runtime/src/execution_server/api_handlers.rs` (if test fixes needed)
- Test: `crates/oris-runtime/src/execution_server/api_handlers.rs` existing suites

**Step 1: Write/adjust failing regression tests**

Ensure existing native endpoints still pass and error payloads remain deterministic:

```rust
#[tokio::test]
async fn evolution_a2a_native_routes_remain_backward_compatible_after_compat_layer() { /* ... */ }
```

**Step 2: Run tests to verify expected failures (if any)**

Run:
- `cargo test -p oris-runtime --features full-evolution-experimental evolution_a2a_handshake_route_accepts_compatible_agent -- --nocapture`
- `cargo test -p oris-runtime --features full-evolution-experimental evolution_a2a_remote_task_session_happy_path_is_executable -- --nocapture`

Expected: PASS; if FAIL, capture exact incompatibility and patch minimally.

**Step 3: Write minimal implementation fixes**

Patch only failing behaviors, avoiding new surface area.

**Step 4: Run full targeted suite**

Run:

```bash
cargo test -p oris-agent-contract -- --nocapture
cargo test -p oris-runtime --features full-evolution-experimental \
  evolution_a2a_handshake_route_accepts_compatible_agent \
  evolution_a2a_handshake_accepts_v1_protocol_only_client \
  evolution_a2a_remote_task_session_happy_path_is_executable \
  evolution_a2a_compat_distribute_and_report_map_to_session_flow \
  evolution_a2a_remote_task_session_rejects_incompatible_protocol_version \
  -- --nocapture
```

Expected: PASS.

**Step 5: Commit**

```bash
git add crates/oris-agent-contract/src/lib.rs crates/oris-runtime/src/execution_server/api_handlers.rs
git commit -m "test: verify evomap a2a compatibility layer and native route regressions"
```

