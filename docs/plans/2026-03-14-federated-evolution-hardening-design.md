# Federated Evolution Attribution and Revocation Hardening Design

## Context

Issue `#232 [EVO26-W7-05][P1] Federated Evolution Attribution and Revocation Hardening` asks us to harden three linked concerns in the federated evolution flow:

- remote attribution must stay stable from import through reuse and revocation
- economics and replay evidence must keep the same remote source identity
- revocation propagation must remain fail-closed on negative paths

The current implementation already records remote source identity in two places:

- `EvolutionEvent::RemoteAssetImported.sender_id`
- `ReplayEconomicsRecorded.evidence.source_sender_id`

That is enough to reconstruct attribution for imported assets, but the behavior is still split across multiple paths:

- import records remote origin
- replay settlement and ROI metrics consume remote origin opportunistically
- explicit `RevokeNotice` processing does not verify that every requested asset actually belongs to the sending node
- replay-failure-triggered revocation emits local revocation state, but its evidence summary does not consistently surface the remote source identity that caused the local quarantine/revocation outcome

This leaves a fail-open gap for federated revocation: one sender can currently request revocation for mixed ownership asset sets, and the kernel will revoke whatever matches locally without proving ownership consistency for the whole request.

## Goals

- Make remote asset ownership reconstruction deterministic from the append-only event log.
- Require explicit remote revocation requests to prove ownership for every referenced asset.
- Keep remote replay failure, economics attribution, and revocation evidence aligned on the same `source_sender_id`.
- Preserve fail-closed behavior when attribution is missing or ambiguous.
- Add regression coverage for cross-node negative paths and audit evidence.

## Non-Goals

- Introducing a new persisted attribution table or sidecar index.
- Changing the public network protocol shape for `EvolutionEnvelope` or `RevokeNotice`.
- Adding partial-success remote revocation semantics.
- Broad federation redesign outside issue `#232`.

## Recommended Approach

Use the existing append-only event stream as the single source of truth and centralize attribution + authorization logic in `crates/oris-evokernel/src/core.rs`.

Why this approach:

1. It is the smallest change that closes the fail-open gap.
2. It avoids introducing a second attribution store that could drift.
3. It lets import, replay economics, implicit revocation, and explicit revocation consume the same ownership reconstruction logic.

## Data Model Strategy

No new wire contract is required.

We will continue to use:

- `EvolutionEvent::RemoteAssetImported { asset_ids, sender_id }` as the canonical remote ownership fact
- `ReplayEconomicsRecorded.evidence.source_sender_id` as replay/economics attribution evidence
- `PromotionEvaluated.evidence.summary` as the human-readable audit surface for state transitions

We will add shared helpers that reconstruct:

- `asset_id -> sender_id`
- `gene_id -> sender_id` by direct import ownership
- `capsule_id -> sender_id` by direct import ownership
- revocation request ownership validation over the full requested asset set

## Architecture

### 1. Shared remote attribution reconstruction

Add a helper that reuses the existing import history reconstruction semantics and returns stable ownership information for requested assets. The helper should read from the append-only store and rely on the same event facts currently used by `remote_publishers_snapshot(...)`.

This keeps a single attribution path for:

- remote selection bias
- replay economics aggregation
- replay-failure revocation evidence
- explicit remote revocation authorization

### 2. Fail-closed revoke authorization

Before `revoke_assets_in_store(...)` writes any revocation events, validate that:

- the notice sender is non-empty after normalization
- every requested asset resolves to a known remote publisher
- every requested asset belongs to the normalized `notice.sender_id`

If any asset is unknown or owned by another sender, reject the whole notice with a validation error and do not append any revocation or quarantine events.

This matches the user-approved boundary for issue `#232`:

- mixed ownership revoke requests are rejected as a whole
- no partial revoke is allowed

### 3. Consistent negative-path audit evidence

When a remote asset fails replay validation and the governor demotes or revokes it, emit evidence summaries that include the remote source identity when available.

The summary should consistently encode:

- phase, for example `replay_failure_revocation`
- `source_sender_id`
- replay failure count
- confidence context

This keeps local state transitions auditable and makes runtime/demo regressions able to assert that the same source identity flows from import to failure handling.

## Component Changes

### `crates/oris-evokernel/src/core.rs`

Add or refactor:

- a helper that resolves remote publishers for requested asset ids from store events
- a helper that validates revoke ownership for a `RevokeNotice`
- a small helper that builds revocation evidence summaries with remote attribution
- updates to replay-failure revocation to use the shared attribution/evidence helper
- updates to `revoke_assets_in_store(...)` to fail closed before mutating the store

### `crates/oris-evokernel/tests/evolution_lifecycle_regression.rs`

Add targeted regressions for:

- successful revoke by the owning remote sender
- fail-closed rejection for mixed-owner revoke requests
- replay-failure revocation evidence that names the remote source

### `crates/oris-runtime/tests/agent_self_evolution_travel_network.rs`

Extend the travel-network scenario so it asserts end-to-end attribution stability across:

- remote import
- remote replay reuse
- remote revoke rejection or success path audit evidence

## Data Flow

### Import and replay path

1. Producer publishes promoted assets.
2. Consumer imports the envelope and records `RemoteAssetImported` with `sender_id`.
3. Consumer replays the imported asset and records replay economics with `source_sender_id`.
4. If replay later fails hard enough to revoke, the transition evidence summary includes the same remote sender.

### Explicit revoke path

1. Remote node sends `RevokeNotice { sender_id, asset_ids, reason }`.
2. Kernel normalizes `sender_id` and asset ids.
3. Kernel resolves ownership for every requested asset from the event log.
4. If any asset is unknown or mismatched, the kernel returns a validation error and appends no revoke events.
5. If all assets match, the kernel appends `GeneRevoked` and `CapsuleQuarantined` events for the affected local projection.

## Error Handling

### Unknown or ambiguous ownership

Behavior: reject the revoke notice.

Reasoning: ownership cannot be proven, so the safe behavior is to do nothing.

### Mixed ownership notice

Behavior: reject the entire revoke notice.

Reasoning: partial success would silently let one sender influence another sender's assets and would weaken the audit model.

### Missing remote source during replay-failure revocation

Behavior: keep the local fail-closed state transition, but emit evidence that attribution was unavailable instead of inventing a sender.

Reasoning: local safety should not depend on attribution completeness, but audit output must remain honest.

## Testing Strategy

### Evokernel regressions

- Owner revoke succeeds and affects the imported remote gene/capsule set.
- Mixed ownership revoke fails and writes no new revoke/quarantine events.
- Remote replay failure revocation emits a `PromotionEvaluated` evidence summary containing `source_sender_id=<node>`.

### Runtime regression

- Extend the travel network test to assert the remote sender survives import, replay reuse, and explicit revoke handling in the audit/event stream.

## Validation Plan

Minimum issue validation from the issue body:

- `cargo test -p oris-evokernel --lib`
- `cargo test -p oris-runtime --test agent_self_evolution_travel_network --features full-evolution-experimental -- --nocapture`

Before release we will also run the standard maintainer release validation flow for the workspace.

## Risks and Mitigations

### Risk: historical stores may rely on older event layouts

Mitigation: ownership reconstruction will continue to derive from the existing `RemoteAssetImported` facts and will not require new persisted fields.

### Risk: audit assertions become brittle if summaries are free-form

Mitigation: keep the evidence summary machine-readable with stable `key=value` fragments for the fields covered by tests.

## Expected Outcome

After this change:

- remote attribution is stable and reconstructable for imported assets
- replay economics and revocation evidence refer to the same sender identity
- explicit remote revocation requests are fail-closed and ownership-checked
- cross-node negative paths are covered by regression tests and produce auditable evidence
