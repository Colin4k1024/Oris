# Oris Evolution Network Protocol (OEN)


> **Implementation Status: Implemented** ✅
Source: https://www.notion.so/317e8a70eec580569ef0ea1713b7e5f6

Last synced: March 2, 2026

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

## 6. Publish Protocol

Triggered when:

```text
Capsule.state == Promoted
```

Flow:

```text
Local Promotion
-> Envelope Creation
-> Peer Broadcast
```

Example endpoint:

```text
POST /evolution/publish
```

## 7. Fetch Protocol

Nodes may request experience using signals such as:

- compiler error signatures
- runtime failures
- performance anomalies

Response returns ranked capsules.

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

## 10. Gossip Propagation Model

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

## 12. Revocation Protocol

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
