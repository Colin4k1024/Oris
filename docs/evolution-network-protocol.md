# Evolution Network Protocol

## Protocol Overview

The Oris Evolution Network transports reusable genes, capsules, and evolution events between peers over a push-pull gossip model. A peer advertises its supported protocols through `/a2a/hello`, exchanges deltas through `/a2a/fetch`, and can hand off queued work through the `/a2a/tasks/*` and `/a2a/work/*` compatibility routes.

At the wire level, the protocol has two layers:

- `EvolutionEnvelope` is the canonical content container for published assets.
- The `/a2a/*` HTTP surface is the compatibility transport used by runtime nodes and external agents.

Current expectations:

- Peers negotiate `oris.a2a` protocol support before using task or work routes.
- Asset replication is cursor-based and resumable.
- Signed envelopes are the preferred transport for remote capsules as of `oris-evolution-network v0.5.0`.
- Receivers may reject unsigned, invalidly signed, or rate-limited capsule deliveries.

## EvolutionEnvelope Schema

`EvolutionEnvelope` is versioned independently from the HTTP route set. The current schema is defined in [crates/oris-evolution-network/src/lib.rs](crates/oris-evolution-network/src/lib.rs).

| Field | Type | Required | Notes |
|---|---|---:|---|
| `protocol` | `string` | yes | Fixed to `oen` for Oris Evolution Network envelopes. |
| `protocol_version` | `string` | yes | Semver-like envelope version string. |
| `message_type` | `publish | fetch | report | revoke` | yes | Logical operation carried by the envelope. |
| `message_id` | `string` | yes | Sender-generated unique identifier. |
| `sender_id` | `string` | yes | Stable node or agent identity. |
| `timestamp` | RFC3339 string | yes | Envelope creation time. |
| `assets` | `NetworkAsset[]` | yes | Mixed list of genes, capsules, and evolution events. |
| `manifest` | `EnvelopeManifest` | no | Publisher metadata and asset integrity summary. |
| `signature` | hex-encoded Ed25519 signature | no at type level, required for hardened remote capsule intake | Signature is calculated over `content_hash`. |
| `content_hash` | hex SHA-256 | yes | Hash of the unsigned envelope payload. |

`EnvelopeManifest` contains:

- `publisher`: human or node publisher identifier
- `sender_id`: sender identity repeated for integrity checks
- `asset_ids`: normalized asset identifiers such as `gene:<id>` and `capsule:<id>`
- `asset_hash`: SHA-256 of the serialized `assets` array

Validation rules implemented by the current runtime:

- `content_hash` must match the envelope payload excluding `signature`
- `manifest.sender_id` must match `sender_id`
- `manifest.asset_ids` and `manifest.asset_hash` must match the actual assets
- hardened receivers verify the Ed25519 signature before admitting remote capsules

## Mandatory Endpoints

The runtime exposes a larger `/a2a/*` surface, but interoperable peers should treat the following as the minimum required compatibility set.

### POST /a2a/hello

Purpose: negotiate protocol compatibility and the enabled capability set.

Request schema: `A2aHandshakeRequest`

| Field | Type |
|---|---|
| `agent_id` | `string` |
| `role` | `AgentRole` |
| `capability_level` | `AgentCapabilityLevel` |
| `supported_protocols` | `A2aProtocol[]` |
| `advertised_capabilities` | `A2aCapability[]` |

Response schema: `A2aCompatHelloResponse` wrapped in `ApiEnvelope`

Important response fields:

- `accepted`
- `negotiated_protocol`
- `enabled_capabilities`
- `payload.node_secret`
- `payload.claim_url`

### POST /a2a/fetch

Purpose: fetch evolution assets and, optionally, queued tasks for the calling peer.

Request schema: `A2aCompatFetchRequest`

| Field | Type |
|---|---|
| `sender_id` or `node_id` | `string` |
| `protocol_version` | `string` |
| `since_cursor` | `string?` |
| `resume_token` | `string?` |
| `asset_type` | `string?` |
| `local_id` | `string?` |
| `content_hash` | `string?` |
| `signals` | `string[]` |
| `search_only` | `bool` |
| `asset_ids` | `string[]` |
| `include_tasks` | `bool` |

Response schema: `A2aCompatFetchResponse` wrapped in `ApiEnvelope`

Response fields include `assets`, `next_cursor`, `resume_token`, `sync_audit`, and optional `tasks`.

### POST /a2a/tasks/distribute

Purpose: open a remote task session and queue work for claim.

Request schema: `A2aCompatDistributeRequest`

Response schema: `A2aCompatDistributeResponse` wrapped in `ApiEnvelope`

### POST /a2a/task/claim

Purpose: claim one queued task dispatch.

Request schema: `A2aCompatClaimRequest`

Response schema: `A2aCompatClaimResponse` wrapped in `ApiEnvelope`

### POST /a2a/task/complete

Purpose: close a claimed task dispatch and submit replay outcome metadata.

Request schema: `A2aCompatTaskCompleteRequest`

Response schema: `A2aCompatReportResponse` wrapped in `ApiEnvelope`

### POST /a2a/work/claim

Purpose: claim an assignment from the compatibility work queue.

Request schema: `A2aCompatWorkClaimRequest`

Response schema: `A2aCompatWorkClaimResponse` wrapped in `ApiEnvelope`

### POST /a2a/work/complete

Purpose: complete a claimed assignment and emit terminal status.

Request schema: `A2aCompatWorkCompleteRequest`

Response schema: `A2aCompatWorkCompleteResponse` wrapped in `ApiEnvelope`

### POST /a2a/heartbeat

Purpose: declare node liveness and receive the currently available work set.

Request schema: `A2aCompatHeartbeatRequest`

Response schema: `A2aCompatHeartbeatResponse` wrapped in `ApiEnvelope`

Response fields include:

- `acknowledged`
- `worker_id`
- `available_work_count`
- `available_work`
- `next_heartbeat_ms`

### POST /a2a/publish

Purpose: import published evolution assets into the remote node.

Request schema: `PublishRequest`

Response schema: `ImportOutcome` wrapped in `ApiEnvelope`

### POST /a2a/revoke

Purpose: revoke previously published assets.

Request schema: `RevokeNotice`

Response schema: `RevokeNotice` wrapped in `ApiEnvelope`

## Signature Format

Signed asset transport uses Ed25519 with hex serialization.

- Private key material is generated through `NodeKeypair::generate()` and persisted to `~/.oris/node.key`
- The persisted file stores the 32-byte Ed25519 secret key as lowercase hex
- `public_key_hex()` returns the 32-byte verifying key as lowercase hex
- `signature` stores the 64-byte Ed25519 signature as lowercase hex
- The signature is computed over `content_hash.as_bytes()`
- `content_hash` is recomputed from the envelope payload with `signature = null`

Receiver behavior for hardened capsule intake:

- missing signature => reject with `missing_signature`
- invalid signature or tampered payload => reject with `invalid_signature`
- per-peer quota exceeded => reject with `rate_limited`

## Metrics Contract

Compliant runtime nodes should expose the following Prometheus metrics.

| Metric | Type | Labels | Description |
|---|---|---|---|
| `oris_network_capsules_received_total` | counter | `peer_id`, `disposition` | Total inbound capsule decisions. `disposition` should distinguish `accept` and `reject`. |
| `oris_network_capsules_sent_total` | counter | `peer_id` | Total capsules sent to a remote peer. |
| `oris_network_gossip_round_duration_ms` | histogram | `peer_id`, `outcome` | End-to-end duration of one gossip synchronization round. |

Recommended auxiliary labels and conventions:

- `peer_id` should use the negotiated sender identity, not a transient socket address
- `disposition` should align with audit log values used in `network_audit.jsonl`
- histogram buckets should be tuned for sub-second and multi-second rounds; the runtime already uses millisecond bucket conventions elsewhere

## Versioning Policy

The protocol uses two related version signals:

- `EvolutionEnvelope.protocol_version` governs the envelope payload contract
- negotiated `A2aProtocol.version` governs the HTTP compatibility layer

Compatibility rules:

- additive fields may be introduced in minor releases if old peers can ignore them safely
- removing or renaming existing fields requires a new advertised protocol version
- receivers should fail closed on unknown critical fields, signature mismatches, and incompatible negotiated protocol versions
- deprecated routes or fields should remain available for at least one minor release after a successor is introduced
- contract JSON and this document should be updated in the same change whenever the `/a2a/*` surface changes

For the current implementation, the compatibility handshake prefers `oris.a2a` version `1.0.0` and can still negotiate the experimental legacy version when needed.