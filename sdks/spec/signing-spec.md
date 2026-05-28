# OEN Signing Specification

## Overview

Oris uses Ed25519 (RFC 8032) signatures for authenticated operations. There are two distinct signing models depending on the service.

## Key Format

- **Private key (seed)**: 32-byte raw Ed25519 seed
- **Public key**: 32-byte Ed25519 verifying key
- **Key encoding varies by context** (see table below)

## Signing Models

### Model A: Hub (Header-Based Signing)

Used by: `POST /hub/nodes`, `PUT /hub/nodes/{node_id}/heartbeat`, `DELETE /hub/nodes/{node_id}`, `POST /hub/events/gene_promoted`

| Aspect | Value |
| ------ | ----- |
| **Signature location** | `X-OEN-Signature` HTTP header |
| **Timestamp location** | Optional `X-OEN-Timestamp` HTTP header (unix seconds) |
| **Payload signed** | Legacy: `{raw_request_body}` bytes. Preferred: `{timestamp}\n{http_method}\n{request_path}\n{raw_request_body}` bytes |
| **Signature encoding** | Base64 (standard, 64 bytes → ~88 chars) |
| **Public key in body** | Base64-encoded (in `public_key` field of RegisterRequest) |
| **Public key storage** | Base64-encoded in node registry |

**Signing algorithm, preferred mode:**

```text
body_bytes = serialize_json(request_body)
timestamp = unix_time_seconds()
payload = concat(
  bytes(str(timestamp)),
  b"\n",
  bytes(http_method),
  b"\n",
  bytes(request_path),
  b"\n",
  body_bytes,
)
signature = ed25519_sign(private_key, payload)
header_value = base64_encode(signature.to_bytes())  // 64 bytes → base64
timestamp_header = str(timestamp)
```

**Signing algorithm, legacy compatibility mode:**

```text
body_bytes = serialize_json(request_body)
signature = ed25519_sign(private_key, body_bytes)
header_value = base64_encode(signature.to_bytes())
```

**Verification algorithm:**

```text
signature_bytes = base64_decode(request.headers["x-oen-signature"])
body_bytes = request.raw_body()
public_key = base64_decode(stored_public_key)  // from registry

if request.headers contains "x-oen-timestamp":
  timestamp = parse_i64(request.headers["x-oen-timestamp"])
  assert timestamp >= 0
  assert timestamp <= now_unix_seconds() + 30
  assert now_unix_seconds() - timestamp <= replay_window_seconds  // configurable, max 3600
  payload = concat(
    bytes(str(timestamp)),
    b"\n",
    bytes(request.method),
    b"\n",
    bytes(request.path),
    b"\n",
    body_bytes,
  )
else:
  payload = body_bytes

ed25519_verify(public_key, payload, signature_bytes)
```

**Special case — registration (POST /hub/nodes):**
The public key is extracted from the request body's `public_key` field (base64-encoded) since no stored key exists yet.

### Model B: Experience Repo (Body-Embedded Signing)

Used by: `POST /experience` (sharing genes/capsules)

| Aspect | Value |
| ------ | ----- |
| **Signature location** | `envelope.signature` field inside request body |
| **Payload signed** | `serde_json::to_vec(&envelope.payload)` (JSON bytes of payload field only) |
| **Signature encoding** | Base64 (server expects base64; see Known Issue below) |
| **Public key registration** | Hex-encoded via `/public-keys` endpoint |
| **Public key storage** | Hex-encoded in key store |
| **HTTP auth header** | `X-Api-Key: {api_key}` (separate from signature) |

**Signing algorithm:**

```text
payload_bytes = serialize_json(envelope.payload)  // compact JSON, no extra whitespace
signature = ed25519_sign(private_key, payload_bytes)
envelope.signature = base64_encode(signature.to_bytes())
```

**Verification algorithm:**

```text
public_key_hex = lookup_public_key(envelope.sender_id)
public_key_bytes = hex_decode(public_key_hex)
payload_bytes = serde_json::to_vec(&envelope.payload)
signature_bytes = base64_decode(envelope.signature)
ed25519_verify(public_key_bytes, payload_bytes, signature_bytes)
```

## Authentication Summary

| Service | Write Auth | Read Auth | Signature Model |
| ------- | ---------- | --------- | --------------- |
| Hub | X-OEN-Signature header | Authorization: Bearer {api_key} | Model A |
| Experience Repo | X-Api-Key header + body signature | None (public reads) | Model B |
| Execution Runtime | Authorization: Bearer {token} | Authorization: Bearer {token} | None (no signing) |

## SDK Implementation Requirements

### Ed25519 Libraries

| Language | Recommended Library |
| -------- | ------------------- |
| Go | `crypto/ed25519` (stdlib) |
| Python | `cryptography` (PyCA) or `nacl` (PyNaCl) |
| TypeScript | `@noble/ed25519` or `tweetnacl` |

### Cross-Language Determinism

To ensure signatures are verifiable across languages:

1. **JSON serialization**: Use compact format (no extra whitespace, no trailing commas)
2. **Key ordering**: For Model B, the payload JSON field ordering must match what serde_json produces (insertion order preserved)
3. **Encoding**: Always use standard base64 (RFC 4648) without padding for signatures in SDK implementations
4. **Timestamp**: Unix seconds in `X-OEN-Timestamp` when using preferred mode

### Golden Test Strategy

Each SDK must pass golden fixture tests that verify:

1. Given a known seed + payload → produces expected signature
2. Given a known public key + payload + signature → verification succeeds
3. Timestamp tolerance check (300s window)

## Known Issue

The current Rust Experience Repo client (`oris-experience-repo/src/client/client.rs`) uses **hex encoding** for signatures, but the server verifier (`verifier.rs`) expects **base64 decoding**. This is a documented P0 bug. SDKs MUST use **base64 encoding** (matching the server verifier).
