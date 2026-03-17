# v0.48.0 - Gossip Sync Engine (P3-01)

## Released Crates

| Crate | Version |
|-------|---------|
| `oris-evolution-network` | 0.4.3 |

## Summary

Adds an operational push-pull gossip sync engine for the evolution network,
including digest generation, request/response synchronization, optional msgpack
framing, and envelope signing stubs for the next security phase.

## Changes

### `oris-evolution-network` 0.4.3

- Added `gossip::GossipSyncEngine` with digest-based push-pull sync.
- Added `GossipConfig`, `GossipDigest`, `GossipDigestEntry`, and `GossipSyncReport`.
- Added threshold-based digest filtering (`broadcast_threshold`, default `0.8`).
- Added fetch round-trip helpers and one-cycle in-process synchronization.
- Added optional `gossip-msgpack` feature via `rmp-serde`.
- Added `EvolutionEnvelope.signature` with a no-op `verify_signature()` stub.

## Validation

- `cargo test -p oris-evolution-network`
- `cargo build --all --release --all-features`

## Resolves

- Closes #303