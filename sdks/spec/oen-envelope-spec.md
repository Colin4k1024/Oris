# OEN Envelope Specification

## Overview

The Oris Evolution Network (OEN) Envelope is the canonical message format for authenticated communication between nodes in the Oris network.

## Structure

```json
{
  "sender_id": "string",
  "message_type": "publish | fetch | feedback",
  "payload": { /* arbitrary JSON */ },
  "signature": "string",
  "timestamp": "string (RFC3339)"
}
```

## Fields

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `sender_id` | string | yes | Unique identifier of the sending node/agent |
| `message_type` | enum | yes | One of: `publish`, `fetch`, `feedback` (snake_case serialization) |
| `payload` | object | yes | Arbitrary JSON payload (content depends on message_type) |
| `signature` | string | yes | Ed25519 signature (see signing-spec.md for encoding) |
| `timestamp` | string | yes | RFC3339 timestamp of message creation |

## Message Types

```
publish  — Publishing a gene or capsule to the network
fetch    — Requesting genes/capsules from the network
feedback — Providing feedback on a gene/capsule
```

Serialization uses `snake_case` (serde rename_all = "snake_case").

## Signature Payload

The signature covers: `serde_json::to_vec(&envelope.payload)`

This means the signature is computed over the **JSON-serialized bytes of the `payload` field only**, not the entire envelope.

## Verification Rules

The server enforces the following during verification:

1. `message_type` must equal `publish` (for share operations)
2. `sender_id` must match the registered agent_id for the public key
3. `timestamp` must be within **300 seconds** (5 minutes) of server time
4. Ed25519 signature must verify against `serde_json::to_vec(&envelope.payload)` using the sender's registered public key

## Public Key Registration

Before sending signed envelopes, a sender must register their public key:

```
POST /public-keys
{
  "sender_id": "my-agent-id",
  "public_key_hex": "hex-encoded-32-byte-ed25519-public-key"
}
```

Public keys are stored as **hex-encoded 32-byte** Ed25519 verifying keys.

## Known Issue: Encoding Mismatch (P0)

The Experience Repo **verifier** expects `signature` to be **base64-encoded**, but the current Rust client writes **hex-encoded** signatures. SDKs should implement **base64 encoding** (matching the verifier's expectation) as the correct behavior.

## Cross-Language Implementation Notes

For deterministic signing across languages:
- Serialize payload to JSON bytes using the same serde_json compact format
- Sign those exact bytes with Ed25519 (RFC 8032)
- Encode the 64-byte signature as **standard base64** (no padding variants accepted)
- Use RFC3339 timestamps with UTC timezone (e.g., `2026-01-15T10:30:00Z`)
