# Oris Evolution Network Protocol (OEN)


> **Implementation Status: In Progress** 🔄
Source: https://www.notion.so/317e8a70eec580569ef0ea1713b7e5f6

Last synced: March 5, 2026

## Current Implementation Snapshot (March 5, 2026)

The current `crates/oris-evolution-network` crate provides protocol contracts:

- `EvolutionEnvelope` with content-hash generation and verification
- `Publish`, `Fetch`, `Report`, and `Revoke` message types
- `Gene`, `Capsule`, and `EvolutionEvent` network asset variants
- request and response structs for publish, fetch, and revoke flows
- experimental re-export through `oris-runtime::evolution_network` behind `evolution-network-experimental`
- runtime `execution-server` exposes experimental HTTP routes:
  - `POST /v1/evolution/publish`
  - `POST /v1/evolution/fetch`
  - `POST /v1/evolution/revoke`
  - `POST /v1/evolution/a2a/handshake` (requires `agent-contract-experimental` in addition to `evolution-network-experimental`)
  - `POST /a2a/hello` (preferred compatibility handshake endpoint)
  - `POST /a2a/fetch` (compatibility asset + task discovery endpoint with `include_tasks`)
  - `POST /a2a/tasks/distribute` and `POST /a2a/tasks/claim` (compatibility queue aliases)
  - `POST /a2a/task/claim` and `POST /a2a/task/complete` (task lifecycle compatibility endpoints)
  - `POST /a2a/work/claim` and `POST /a2a/work/complete` (worker-pool compatibility endpoints)
  - `POST /a2a/heartbeat` (compatibility worker keepalive + `available_work`)
  - `POST /evolution/a2a/hello` (EvoMap-compatible handshake alias)
  - `POST /evolution/a2a/tasks/distribute` (EvoMap-compatible queue/distribute alias)
  - `POST /evolution/a2a/tasks/claim` (EvoMap-compatible claim alias)
  - `POST /evolution/a2a/tasks/report` (EvoMap-compatible progress/complete alias)
  - `POST /v1/evolution/a2a/sessions/start`
  - `POST /v1/evolution/a2a/sessions/:session_id/dispatch`
  - `POST /v1/evolution/a2a/sessions/:session_id/progress`
  - `POST /v1/evolution/a2a/sessions/:session_id/complete`
  - `GET /v1/evolution/a2a/sessions/:session_id`
  - `GET /v1/evolution/a2a/sessions/:sender_id/replicate`
  - `POST /v1/evolution/a2a/sessions/replicate`
  - `GET /v1/evolution/a2a/tasks/:task_id/lifecycle` (requires `agent-contract-experimental` in addition to `evolution-network-experimental`)
- when `agent-contract-experimental` is enabled, publish/fetch/revoke calls require a prior successful handshake for the same `sender_id` and a negotiated matching capability
- when both `agent-contract-experimental` and `sqlite-persistence` are enabled, negotiated handshake sessions are persisted in runtime storage and can be reused after process restart
- runtime task execution, replay outcomes, and worker supervised acknowledgements now emit A2A lifecycle transitions that can be queried by task id
- remote A2A task session transitions follow `Started -> Dispatched -> InProgress* -> Completed|Failed|Cancelled`
- session completion payloads are normalized into `ReplayFeedback` (`SkipPlanner` or `PlanFallback`) so remote results can feed replay-aware evolution decisions
- remote task session protocol versions are strictly checked; incompatible versions return deterministic `400` errors
- compatibility A2A routes accept `sender_id` or `node_id`; when `protocol_version` is omitted they default to `oris.a2a@1.0.0`
- runtime now enforces agent-managed privilege profiles (`observer`/`operator`/`governor`) across evolution and A2A session endpoints, with audit logs capturing principal, capability, and allow/deny reasons
- negotiated A2A sessions can be explicitly replicated across nodes via export/import APIs, enabling cross-node reuse after handshake on the source node
- runtime metrics now expose compatibility queue/claim/report telemetry:
  - `oris_a2a_task_queue_depth`
  - `oris_a2a_task_claim_latency_ms`
  - `oris_a2a_task_lease_expired_total`
  - `oris_a2a_report_to_capture_latency_ms`
  - `oris_a2a_fetch_total`
  - `oris_a2a_task_claim_total`
  - `oris_a2a_task_complete_total`
  - `oris_a2a_work_claim_total`
  - `oris_a2a_work_complete_total`
  - `oris_a2a_heartbeat_total`

Not yet implemented in the checked-in crate:

- peer discovery or gossip propagation
- automatic publish gating based on promoted asset state
- remote trust execution or validation pipelines

## Compatibility Endpoint Examples (Current Experimental Shape)

Compatibility handshake:

```http
POST /evolution/a2a/hello
content-type: application/json

{
  "agent_id": "agent-compat-1",
  "role": "Planner",
  "capability_level": "A4",
  "supported_protocols": [
    { "name": "oris.a2a", "version": "1.0.0" }
  ],
  "advertised_capabilities": ["Coordination", "SupervisedDevloop", "ReplayFeedback"]
}
```

Compatibility distribute using alias fields (`node_id`, `task_description`) and implicit protocol default:

```http
POST /evolution/a2a/tasks/distribute
content-type: application/json

{
  "node_id": "agent-compat-1",
  "task_id": "task-compat-1",
  "task_description": "Fix failing CI job",
  "dispatch_id": "dispatch-task-compat-1",
  "summary": "queued for compat execution"
}
```

Compatibility claim (protocol defaults to `1.0.0` when omitted):

```http
POST /evolution/a2a/tasks/claim
content-type: application/json

{
  "node_id": "agent-compat-1"
}
```

Compatibility report accepts both canonical and alias statuses (`running`/`in_progress`, `succeeded`/`completed`):

```http
POST /evolution/a2a/tasks/report
content-type: application/json

{
  "node_id": "agent-compat-1",
  "task_id": "task-compat-1",
  "status": "completed",
  "summary": "task finished",
  "used_capsule": true,
  "capsule_id": "capsule-compat-1",
  "reasoning_steps_avoided": 3,
  "task_class_id": "ci.fix",
  "task_label": "Fix CI"
}
```

Compatibility fetch + task/work + heartbeat flow endpoints (`/a2a/*`):

```http
POST /a2a/fetch
content-type: application/json

{
  "sender_id": "agent-compat-1",
  "protocol_version": "1.0.0",
  "include_tasks": true
}
```

```http
POST /a2a/task/claim
content-type: application/json

{
  "sender_id": "agent-compat-1",
  "protocol_version": "1.0.0"
}
```

```http
POST /a2a/task/complete
content-type: application/json

{
  "sender_id": "agent-compat-1",
  "protocol_version": "1.0.0",
  "task_id": "task-compat-1",
  "status": "succeeded",
  "summary": "task finished"
}
```

```http
POST /a2a/work/claim
content-type: application/json

{
  "sender_id": "agent-compat-1",
  "protocol_version": "1.0.0"
}
```

```http
POST /a2a/work/complete
content-type: application/json

{
  "sender_id": "agent-compat-1",
  "protocol_version": "1.0.0",
  "assignment_id": "session-compat-1",
  "status": "succeeded",
  "summary": "work assignment finished"
}
```

```http
POST /a2a/heartbeat
content-type: application/json

{
  "sender_id": "agent-compat-1",
  "protocol_version": "1.0.0",
  "metadata": { "worker_mode": "compat" }
}
```

## End-to-End Compatibility Operation Runbook (fetch -> task/work claim -> complete -> heartbeat)

### Preconditions

- Start `execution_server` with compatibility routes available:

```bash
cargo run -p oris-runtime --example execution_server --features "full-evolution-experimental execution-server sqlite-persistence"
```

- Install `curl` and `jq`.
- Ensure the same owner identity is used across distribute, claim, and report (the current implementation enforces lease ownership by sender).

### Dual-mode auth template (local or bearer token)

```bash
export BASE_URL="${BASE_URL:-http://127.0.0.1:8080}"
export AUTH_TOKEN="${AUTH_TOKEN:-}"
export REQUEST_ID_PREFIX="${REQUEST_ID_PREFIX:-compat-e2e}"
export SENDER_ID="${SENDER_ID:-compat-e2e-agent-1}"
export TASK_ID="${TASK_ID:-compat-e2e-task-1}"
export WORK_TASK_ID="${WORK_TASK_ID:-compat-e2e-task-2}"
export PROTO="${PROTO:-1.0.0}"
export HELLO_BASE="${HELLO_BASE:-/a2a}"
export DISTRIBUTE_BASE="${DISTRIBUTE_BASE:-/a2a}"
# Legacy alias mode (optional):
# export HELLO_BASE="/evolution/a2a"
# export DISTRIBUTE_BASE="/evolution/a2a"

AUTH_HEADERS=(-H "content-type: application/json")
if [ -n "${AUTH_TOKEN}" ]; then
  AUTH_HEADERS+=(-H "authorization: Bearer ${AUTH_TOKEN}")
fi
```

### Step 1: hello (handshake)

```bash
hello_resp="$(curl -sS -X POST "${BASE_URL}${HELLO_BASE}/hello" \
  "${AUTH_HEADERS[@]}" \
  -H "x-request-id: ${REQUEST_ID_PREFIX}-hello" \
  -d "{
    \"agent_id\":\"${SENDER_ID}\",
    \"role\":\"Planner\",
    \"capability_level\":\"A2\",
    \"supported_protocols\":[{\"name\":\"oris.a2a\",\"version\":\"${PROTO}\"}],
    \"advertised_capabilities\":[\"Coordination\",\"SupervisedDevloop\",\"ReplayFeedback\",\"EvolutionFetch\"]
  }")"
```

Expected response key fields (`jq`):

```bash
echo "${hello_resp}" | jq -e '.data.accepted == true'
echo "${hello_resp}" | jq -e '.data.negotiated_protocol.version == env.PROTO'
```

### Step 2: distribute two tasks (one for task flow, one for work flow)

```bash
distribute_task_resp="$(curl -sS -X POST "${BASE_URL}${DISTRIBUTE_BASE}/tasks/distribute" \
  "${AUTH_HEADERS[@]}" \
  -H "x-request-id: ${REQUEST_ID_PREFIX}-distribute-task" \
  -d "{
    \"sender_id\":\"${SENDER_ID}\",
    \"protocol_version\":\"${PROTO}\",
    \"task_id\":\"${TASK_ID}\",
    \"task_summary\":\"Compat E2E task-flow item\",
    \"dispatch_id\":\"dispatch-${TASK_ID}\",
    \"summary\":\"queued for task claim -> complete flow\"
  }")"

distribute_work_resp="$(curl -sS -X POST "${BASE_URL}${DISTRIBUTE_BASE}/tasks/distribute" \
  "${AUTH_HEADERS[@]}" \
  -H "x-request-id: ${REQUEST_ID_PREFIX}-distribute-work" \
  -d "{
    \"sender_id\":\"${SENDER_ID}\",
    \"protocol_version\":\"${PROTO}\",
    \"task_id\":\"${WORK_TASK_ID}\",
    \"task_summary\":\"Compat E2E work-flow item\",
    \"dispatch_id\":\"dispatch-${WORK_TASK_ID}\",
    \"summary\":\"queued for work claim -> complete flow\"
  }")"
```

Expected response key fields (`jq`):

```bash
echo "${distribute_task_resp}" | jq -e '.data.state == "Dispatched"'
echo "${distribute_work_resp}" | jq -e '.data.state == "Dispatched"'
```

### Step 3: fetch + include_tasks

```bash
fetch_resp="$(curl -sS -X POST "${BASE_URL}/a2a/fetch" \
  "${AUTH_HEADERS[@]}" \
  -H "x-request-id: ${REQUEST_ID_PREFIX}-fetch" \
  -d "{
    \"sender_id\":\"${SENDER_ID}\",
    \"protocol_version\":\"${PROTO}\",
    \"include_tasks\":true
  }")"
```

Expected response key fields (`jq`):

```bash
echo "${fetch_resp}" | jq -e '.data.tasks | length >= 2'
echo "${fetch_resp}" | jq -e '.data.tasks | map(.task_id) | index(env.TASK_ID) != null'
echo "${fetch_resp}" | jq -e '.data.tasks | map(.task_id) | index(env.WORK_TASK_ID) != null'
```

### Step 4: task claim

```bash
task_claim_resp="$(curl -sS -X POST "${BASE_URL}/a2a/task/claim" \
  "${AUTH_HEADERS[@]}" \
  -H "x-request-id: ${REQUEST_ID_PREFIX}-task-claim" \
  -d "{
    \"sender_id\":\"${SENDER_ID}\",
    \"protocol_version\":\"${PROTO}\"
  }")"
CLAIMED_TASK_ID="$(echo "${task_claim_resp}" | jq -r '.data.task.task_id')"
CLAIMED_SESSION_ID="$(echo "${task_claim_resp}" | jq -r '.data.task.session_id')"
```

Expected response key fields (`jq`):

```bash
echo "${task_claim_resp}" | jq -e '.data.claimed == true'
test -n "${CLAIMED_TASK_ID}" && [ "${CLAIMED_TASK_ID}" != "null" ]
test -n "${CLAIMED_SESSION_ID}" && [ "${CLAIMED_SESSION_ID}" != "null" ]
```

### Step 5: task complete

```bash
task_complete_resp="$(curl -sS -X POST "${BASE_URL}/a2a/task/complete" \
  "${AUTH_HEADERS[@]}" \
  -H "x-request-id: ${REQUEST_ID_PREFIX}-task-complete" \
  -d "{
    \"sender_id\":\"${SENDER_ID}\",
    \"protocol_version\":\"${PROTO}\",
    \"task_id\":\"${CLAIMED_TASK_ID}\",
    \"status\":\"succeeded\",
    \"summary\":\"compat task completed\"
  }")"
```

Expected response key fields (`jq`):

```bash
echo "${task_complete_resp}" | jq -e '.data.state == "Completed"'
echo "${task_complete_resp}" | jq -e '.data.terminal_state == "Succeeded"'
```

### Step 6: work claim

```bash
work_claim_resp="$(curl -sS -X POST "${BASE_URL}/a2a/work/claim" \
  "${AUTH_HEADERS[@]}" \
  -H "x-request-id: ${REQUEST_ID_PREFIX}-work-claim" \
  -d "{
    \"sender_id\":\"${SENDER_ID}\",
    \"protocol_version\":\"${PROTO}\"
  }")"
ASSIGNMENT_ID="$(echo "${work_claim_resp}" | jq -r '.data.assignment.assignment_id')"
WORK_TASK_ID_CLAIMED="$(echo "${work_claim_resp}" | jq -r '.data.assignment.task_id')"
```

Expected response key fields (`jq`):

```bash
echo "${work_claim_resp}" | jq -e '.data.claimed == true'
test -n "${ASSIGNMENT_ID}" && [ "${ASSIGNMENT_ID}" != "null" ]
test -n "${WORK_TASK_ID_CLAIMED}" && [ "${WORK_TASK_ID_CLAIMED}" != "null" ]
```

### Step 7: work complete

```bash
work_complete_resp="$(curl -sS -X POST "${BASE_URL}/a2a/work/complete" \
  "${AUTH_HEADERS[@]}" \
  -H "x-request-id: ${REQUEST_ID_PREFIX}-work-complete" \
  -d "{
    \"sender_id\":\"${SENDER_ID}\",
    \"protocol_version\":\"${PROTO}\",
    \"assignment_id\":\"${ASSIGNMENT_ID}\",
    \"task_id\":\"${WORK_TASK_ID_CLAIMED}\",
    \"status\":\"succeeded\",
    \"summary\":\"compat work assignment completed\"
  }")"
```

Expected response key fields (`jq`):

```bash
echo "${work_complete_resp}" | jq -e '.data.state == "Completed"'
echo "${work_complete_resp}" | jq -e '.data.terminal_state == "Succeeded"'
```

### Step 8: worker heartbeat (available_work should be empty after both completions)

```bash
heartbeat_resp="$(curl -sS -X POST "${BASE_URL}/a2a/heartbeat" \
  "${AUTH_HEADERS[@]}" \
  -H "x-request-id: ${REQUEST_ID_PREFIX}-heartbeat" \
  -d "{
    \"sender_id\":\"${SENDER_ID}\",
    \"protocol_version\":\"${PROTO}\",
    \"metadata\":{\"worker_mode\":\"compat-runbook\"}
  }")"
```

Expected response key fields (`jq`):

```bash
echo "${heartbeat_resp}" | jq -e '.data.acknowledged == true'
echo "${heartbeat_resp}" | jq -e '.data.metadata_accepted == true'
echo "${heartbeat_resp}" | jq -e '.data.available_work_count == 0'
```

### Step 9 (optional): snapshot + lifecycle verification

```bash
snapshot_resp="$(curl -sS "${BASE_URL}/v1/evolution/a2a/sessions/${CLAIMED_SESSION_ID}?sender_id=${SENDER_ID}&protocol_version=${PROTO}" \
  "${AUTH_HEADERS[@]}" \
  -H "x-request-id: ${REQUEST_ID_PREFIX}-snapshot")"

lifecycle_resp="$(curl -sS "${BASE_URL}/v1/evolution/a2a/tasks/${CLAIMED_TASK_ID}/lifecycle?sender_id=${SENDER_ID}&protocol_version=${PROTO}" \
  "${AUTH_HEADERS[@]}" \
  -H "x-request-id: ${REQUEST_ID_PREFIX}-lifecycle")"
```

Expected response key fields (`jq`):

```bash
echo "${snapshot_resp}" | jq -e '.data.state == "Completed"'
echo "${snapshot_resp}" | jq -e '.data.result.terminal_state == "Succeeded"'
echo "${lifecycle_resp}" | jq -e '.data.events[-1].state == "Succeeded"'
```

### Step 10 (optional): final claim should be empty

```bash
final_claim_resp="$(curl -sS -X POST "${BASE_URL}/a2a/task/claim" \
  "${AUTH_HEADERS[@]}" \
  -H "x-request-id: ${REQUEST_ID_PREFIX}-claim-final" \
  -d "{
    \"sender_id\":\"${SENDER_ID}\",
    \"protocol_version\":\"${PROTO}\"
  }")"
```

Expected response key fields (`jq`):

```bash
echo "${final_claim_resp}" | jq -e '.data.claimed == false'
```

### Canonical-field equivalents

- `HELLO_BASE` / `DISTRIBUTE_BASE` can be switched between `/a2a` and `/evolution/a2a`.
- `/evolution/a2a/*` remains the legacy compatibility alias family for `hello` + `tasks/*`.
- `/a2a/*` is the preferred compatibility family and is required for `fetch`, `task/*`, `work/*`, and `heartbeat`.
- `node_id` is accepted as alias for `sender_id`.
- `task_description` is accepted as alias for `task_summary`.
- `status: in_progress` maps to canonical `running`.
- `status: completed` maps to canonical `succeeded`.

### Common failures and triage

- No handshake before compatibility task calls:
  response is `403 forbidden` with message `a2a handshake required before calling evolution routes`.
- Missing sender (`sender_id` and `node_id` both empty/missing):
  response is `400` with details field `a2a_error_code: ValidationFailed`.
- Incompatible protocol version:
  response is `400` with message `incompatible a2a task session protocol version`.
- Missing negotiated `EvolutionFetch` capability when calling `/a2a/fetch`:
  response is `403` with message `negotiated capabilities do not allow this evolution action`.
- Lease owner mismatch during task/work completion:
  response is `403` with message `compat task lease is owned by another claimer` or `a2a work assignment is owned by another claimer`.

## Related Documents

- [evolution.md](evolution.md)
- [governor.md](governor.md)
- [economics.md](economics.md)

## 1. Purpose

The Oris Evolution Network enables multiple Oris nodes to share verified
evolutionary intelligence.

Nodes may:

- publish successful evolution assets
- inherit verified experience from peers
- accelerate capability acquisition
- form distributed collective intelligence

## 2. Design Principles

### 2.1 Local Sovereignty

Each node remains autonomous. Remote assets:

- are never trusted automatically
- must pass local validation
- cannot bypass governor control

### 2.2 Execution-Based Trust

Trust derives only from:

```text
verified execution success
```

### 2.3 Eventual Evolution Consistency

The network does not require global synchronization.

### 2.4 Safety Before Speed

Inheritance must never compromise system stability.

## 3. Network Architecture

Minimal topology:

```text
Oris Node A <-> Oris Node B
      ^           ^
      |           |
Oris Node C <-> Oris Node D
```

Recommended model:

- peer-to-peer mesh
- gossip-style propagation
- partial peer connectivity

## 4. Evolution Envelope

All communication uses a standardized envelope:

```rust
struct EvolutionEnvelope {
    protocol: String,
    protocol_version: String,
    message_type: MessageType,
    message_id: String,
    sender_id: NodeId,
    timestamp: Timestamp,
    assets: Vec<Asset>,
}
```

Message types:

```rust
enum MessageType {
    Publish,
    Fetch,
    Report,
    Revoke,
}
```

## 5. Evolution Assets Over Network

Transferable assets:

```rust
enum Asset {
    Gene,
    Capsule,
    EvolutionEvent,
}
```

Only promoted assets may be published.

## 6. Publish Protocol (Target)

Target design assumes publish is triggered when:

```text
Capsule.state == Promoted
```

Flow:

```text
Local Promotion
-> Envelope Creation
-> Peer Broadcast
```

An endpoint such as the following is a future transport shape, not a currently shipped API:

```text
POST /evolution/publish
```

## 7. Fetch Protocol (Target)

Nodes may request experience using signals such as:

- compiler error signatures
- runtime failures
- performance anomalies

The current crate only defines fetch query and response payload types.

## 8. Remote Asset Lifecycle

Incoming assets:

```text
Remote Asset
-> Candidate Pool
-> Sandbox Validation
-> Local Replay
-> Governor Approval
-> Promotion
```

## 9. Trust Model

### 9.1 Content Addressing

All assets include deterministic hashes:

```text
asset_id = sha256(canonical_asset)
```

### 9.2 Node Reputation (Optional)

```rust
struct NodeReputation {
    reuse_success_rate: f32,
    validated_assets: u64,
}
```

Reputation influences selection weighting only.

### 9.3 Quarantine Enforcement

Mandatory checks:

- schema validation
- sandbox execution
- deterministic replay

## 10. Gossip Propagation Model (Planned)

Recommended strategy:

```text
node publishes -> random peers -> further propagation
```

Advantages:

- scalability
- resilience
- decentralization
- reduced coordination cost

## 11. Conflict Handling

Duplicate or competing strategies are resolved through:

- local success rate
- replay validation
- governor evaluation

No global conflict authority is required.

## 12. Revocation Protocol (Target)

Nodes may revoke published assets with `MessageType::Revoke`.

Reasons:

- regression detected
- unsafe behavior
- validation failure

Receiving nodes downgrade affected assets.

## 13. Security Model

Threats:

- malicious evolution injection
- poisoned strategies
- replay inconsistency
- spam asset flooding

Mitigations:

- local validation
- governor enforcement
- blast radius limits
- promotion thresholds

## 14. Observability

Recommended metrics:

- assets published
- assets adopted
- reuse success rate
- remote validation failures
- propagation latency

## 15. Expected Network Emergence

- Stage 1: local learning
- Stage 2: experience sharing
- Stage 3: organizational intelligence
- Stage 4: collective evolution

## 16. Repository Integration

Recommended module:

```text
oris/
`- network/
   |- envelope/
   |- transport/
   |- peer/
   |- publish/
   |- fetch/
   `- quarantine/
```

## 17. Future Extensions

- reputation economies
- validator consensus
- cross-organization federation
- evolution marketplaces

## 18. Non-Goals

OEN does not:

- centralize intelligence
- bypass local governance
- guarantee correctness
- replace validation mechanisms

## 19. Vision

```text
execution success
-> transferable intelligence
-> distributed learning
```

This yields scalable intelligence growth independent of model size.
