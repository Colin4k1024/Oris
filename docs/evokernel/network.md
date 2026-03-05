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
- when `agent-contract-experimental` is enabled, publish/fetch/revoke calls require a prior successful handshake for the same `sender_id` and a negotiated matching capability
- when both `agent-contract-experimental` and `sqlite-persistence` are enabled, negotiated handshake sessions are persisted in runtime storage and can be reused after process restart

Not yet implemented in the checked-in crate:

- peer discovery or gossip propagation
- automatic publish gating based on promoted asset state
- remote trust execution or validation pipelines

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
