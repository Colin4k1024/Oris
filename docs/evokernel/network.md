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

## End-to-End Compatibility Operation Runbook (distribute -> claim -> report)

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
export PROTO="${PROTO:-1.0.0}"

AUTH_HEADERS=(-H "content-type: application/json")
if [ -n "${AUTH_TOKEN}" ]; then
  AUTH_HEADERS+=(-H "authorization: Bearer ${AUTH_TOKEN}")
fi
```

### Step 1: hello (handshake)

```bash
hello_resp="$(curl -sS -X POST "${BASE_URL}/evolution/a2a/hello" \
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

### Step 2: distribute (alias fields: node_id + task_description)

```bash
distribute_resp="$(curl -sS -X POST "${BASE_URL}/evolution/a2a/tasks/distribute" \
  "${AUTH_HEADERS[@]}" \
  -H "x-request-id: ${REQUEST_ID_PREFIX}-distribute" \
  -d "{
    \"node_id\":\"${SENDER_ID}\",
    \"protocol_version\":\"${PROTO}\",
    \"task_id\":\"${TASK_ID}\",
    \"task_description\":\"Compat E2E execution task\",
    \"dispatch_id\":\"dispatch-${TASK_ID}\",
    \"summary\":\"queued by compat runbook\"
  }")"
SESSION_ID="$(echo "${distribute_resp}" | jq -r '.data.session_id')"
```

Expected response key fields (`jq`):

```bash
echo "${distribute_resp}" | jq -e '.data.state == "Dispatched"'
echo "${distribute_resp}" | jq -e '.data.task_id == env.TASK_ID'
test -n "${SESSION_ID}" && [ "${SESSION_ID}" != "null" ]
```

### Step 3: claim

```bash
claim_resp="$(curl -sS -X POST "${BASE_URL}/evolution/a2a/tasks/claim" \
  "${AUTH_HEADERS[@]}" \
  -H "x-request-id: ${REQUEST_ID_PREFIX}-claim" \
  -d "{
    \"node_id\":\"${SENDER_ID}\",
    \"protocol_version\":\"${PROTO}\"
  }")"
CLAIMED_SESSION_ID="$(echo "${claim_resp}" | jq -r '.data.task.session_id // empty')"
```

Expected response key fields (`jq`):

```bash
echo "${claim_resp}" | jq -e '.data.claimed == true'
echo "${claim_resp}" | jq -e '.data.task.task_id == env.TASK_ID'
test "${CLAIMED_SESSION_ID}" = "${SESSION_ID}"
```

### Step 4: report running (status alias: in_progress)

```bash
report_running_resp="$(curl -sS -X POST "${BASE_URL}/evolution/a2a/tasks/report" \
  "${AUTH_HEADERS[@]}" \
  -H "x-request-id: ${REQUEST_ID_PREFIX}-report-running" \
  -d "{
    \"node_id\":\"${SENDER_ID}\",
    \"protocol_version\":\"${PROTO}\",
    \"task_id\":\"${TASK_ID}\",
    \"status\":\"in_progress\",
    \"summary\":\"compat task in progress\",
    \"progress_pct\":55,
    \"retryable\":false
  }")"
```

Expected response key fields (`jq`):

```bash
echo "${report_running_resp}" | jq -e '.data.state == "InProgress"'
```

### Step 5: report completed (status alias: completed)

```bash
report_complete_resp="$(curl -sS -X POST "${BASE_URL}/evolution/a2a/tasks/report" \
  "${AUTH_HEADERS[@]}" \
  -H "x-request-id: ${REQUEST_ID_PREFIX}-report-complete" \
  -d "{
    \"node_id\":\"${SENDER_ID}\",
    \"protocol_version\":\"${PROTO}\",
    \"task_id\":\"${TASK_ID}\",
    \"status\":\"completed\",
    \"summary\":\"compat task completed\",
    \"retryable\":false,
    \"used_capsule\":true,
    \"capsule_id\":\"capsule-${TASK_ID}\",
    \"reasoning_steps_avoided\":2,
    \"task_class_id\":\"compat.e2e\",
    \"task_label\":\"Compat E2E task\"
  }")"
```

Expected response key fields (`jq`):

```bash
echo "${report_complete_resp}" | jq -e '.data.state == "Completed"'
echo "${report_complete_resp}" | jq -e '.data.terminal_state == "Succeeded"'
```

### Step 6: snapshot query

```bash
snapshot_resp="$(curl -sS "${BASE_URL}/v1/evolution/a2a/sessions/${SESSION_ID}?sender_id=${SENDER_ID}&protocol_version=${PROTO}" \
  "${AUTH_HEADERS[@]}" \
  -H "x-request-id: ${REQUEST_ID_PREFIX}-snapshot")"
```

Expected response key fields (`jq`):

```bash
echo "${snapshot_resp}" | jq -e '.data.state == "Completed"'
echo "${snapshot_resp}" | jq -e '.data.result.terminal_state == "Succeeded"'
```

### Step 7: lifecycle query

```bash
lifecycle_resp="$(curl -sS "${BASE_URL}/v1/evolution/a2a/tasks/${TASK_ID}/lifecycle?sender_id=${SENDER_ID}&protocol_version=${PROTO}" \
  "${AUTH_HEADERS[@]}" \
  -H "x-request-id: ${REQUEST_ID_PREFIX}-lifecycle")"
```

Expected response key fields (`jq`):

```bash
echo "${lifecycle_resp}" | jq -e '.data.events[-1].state == "Succeeded"'
```

### Step 8 (optional): claim again after completion

```bash
final_claim_resp="$(curl -sS -X POST "${BASE_URL}/evolution/a2a/tasks/claim" \
  "${AUTH_HEADERS[@]}" \
  -H "x-request-id: ${REQUEST_ID_PREFIX}-claim-final" \
  -d "{
    \"node_id\":\"${SENDER_ID}\",
    \"protocol_version\":\"${PROTO}\"
  }")"
```

Expected response key fields (`jq`):

```bash
echo "${final_claim_resp}" | jq -e '.data.claimed == false'
```

### Canonical-field equivalents

- `node_id` is accepted as alias for `sender_id`.
- `task_description` is accepted as alias for `task_summary`.
- `status: in_progress` maps to canonical `running`.
- `status: completed` maps to canonical `succeeded`.

### Common failures and triage

- No handshake before distribute/claim/report:
  response is `403 forbidden` with message `a2a handshake required before calling evolution routes`.
- Missing sender (`sender_id` and `node_id` both empty/missing):
  response is `400` with details field `a2a_error_code: ValidationFailed`.
- Incompatible protocol version:
  response is `400` with message `incompatible a2a task session protocol version`.
- Lease owner mismatch during report:
  response is `403` with message `compat task lease is owned by another claimer`.

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
