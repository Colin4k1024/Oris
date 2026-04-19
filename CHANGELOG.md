# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [Unreleased]

---

## oris-experience-repo [0.3.0] — 2026-04-19

### Added
- Ed25519 signature verification fully enabled end-to-end (`OenVerifier::verify_envelope`)
- PKI public key registry with SQLite persistence, versioning, rotation, and soft-delete
- `KeyStore::register_public_key`, `get_public_key`, `revoke_public_key` APIs
- Rate limiting now covers all endpoints (GET /experience, GET/POST/DELETE /keys, POST /keys/{id}/rotate)
- 13 Ed25519 integration tests: valid signature passes, invalid/tampered/replayed/revoked all rejected

### Security
- Signature verification no longer deferred — all OEN envelopes are validated against registered keys
- Replay attack protection with 5-minute window enforced in `OenVerifier`
- Revoked keys immediately rejected; rotation generates new key version atomically

### Changed
- `OenVerifier` now requires a `KeyStore` reference for public key lookup
- Upgraded from v0.2.0; all v0.2.0 API surfaces remain backward-compatible

---

## oris-experience-repo [0.2.0] — 2026-04-14

### Added
- PKI key service with Ed25519 public key registry (`key_service` module)
- OEN Envelope verifier with signature validation infrastructure (`oen` module)
- Rate limiting middleware via `governor` crate (GET 100/min, POST 30/min, Keys 20/min)
- HTTP API server with Axum 0.8 and full OpenAPI 3.0.3 specification
- API Key management endpoints: `GET/POST /keys`, `DELETE /keys/{id}`, `POST /keys/{id}/rotate`
- Experience fetch endpoint with cursor pagination and confidence filtering
- OEN Envelope submission endpoint (`POST /experience`)
- 25 unit tests + 13 integration tests (38 total)
- Published as standalone crate to crates.io

### Security
- Ed25519 signing infrastructure in place; signature verification deferred pending full PKI rollout
- Rate limiting covers POST /experience; remaining endpoints hardened in next release
- 5-minute replay window accepted as known risk until signing is enabled

---

## oris-runtime [0.61.0] — 2026-04

### Added
- A2A Economic Lifecycle: `POST /a2a/service/register`, `GET /a2a/service/list`, `GET /a2a/service/:id`
- A2A Bid workflow: `POST /a2a/bid/submit`, `GET /a2a/bid/:id`, `POST /a2a/bid/evaluate`
- A2A Dispute rule querying: `GET /a2a/dispute/rule`
- Deterministic bid evaluation with `highest_bid` / `lowest_bid` strategies, `schema_version: "v1"` for auditability
- 16 new integration tests

---

## oris-runtime [0.60.0] — 2026-04

### Added
- A2A Project Workflow: create, detail, state transitions (active/paused/completed), per-project suggestions, list
- `EvomapProjectRecord.lifecycle_state` field
- 11 new integration tests

---

## oris-runtime [0.59.0] — 2026-03

### Added
- A2A Council Workflow: session open/close, proposal submission, voting, execution with quorum enforcement
- 13 new integration tests covering full council lifecycle

---

## oris-runtime [0.58.0] — 2026-03

### Added
- A2A Asset Detail & Governance: verify, vote, audit trail, reviews shape
- Role-based governance enforcement (worker role blocked on verify/vote)
- 13 new integration tests

---

## oris-runtime [0.57.0] — 2026-03

### Added
- A2A Asset Discovery API: search, ranked, explore, recommended, trending, categories
- Deterministic ranking for same inputs; pagination via `limit`/`offset`
- 13 new integration tests

---

## oris-runtime [0.56.0] — 2026-03

### Added
- A2A Task Lifecycle: submit, list, get, my, eligible-count, release, ask
- Fixed `/v1/evolution/a2a/tasks/:task_id/lifecycle` route registration under `a2a-production`
- 18 new integration tests

---

## oris-runtime [0.55.0] — 2026-02

### Added
- A2A Protocol Core Semantics: validate, report, decision, revoke, policy/model-tiers
- 16 unit tests covering the `a2a-production` feature surface

---

## oris-runtime [0.54.0] — 2026-02

### Added
- Fail-closed Autonomous Merge and Release Gate (`MergeGate`, `ExtendedReleaseGate`, `GatedPublishGate`)
- Approved task classes for autonomous lane: `missing-import`, `type-mismatch`, `test-failure`
- oris-orchestrator bumped to 0.5.0; 44 new tests

---

## oris-runtime [0.53.0] — 2026-02

### Added
- Fail-closed autonomous PR lane with deterministic branch and evidence-backed PR artifact preparation
- Machine-readable autonomous PR lane contracts
- EvoKernel autonomous PR lane gate for low-risk docs and lint tasks

---

## oris-runtime [0.52.0] — 2026-01

### Added
- Continuous Confidence Revalidation

---

## oris-kernel [0.2.x] — Kernel K1–K5 Hardening (2026 Q1)

### K5 — Lease-based Finalization
- Lease-based finalization with zero-data-loss recovery
- Context-aware scheduler with weighted priority dispatch
- Backpressure engine

### K4 — Plugin System
- 9 plugin categories (Node, Tool, Memory, LLMAdapter, Scheduler, Checkpoint, Effect, Observer, Governor)
- Determinism contracts, resource limits, version negotiation
- `PluginRegistry` with deterministic declaration surface

### K3 — Interrupt & Resume
- Interrupt object and suspension state machine
- Replay-based resume from suspension point

### K2 — Canonical Log Store
- Canonical event log store with replay cursor
- Replay verification and branch replay support

### K1 — ExecutionStep Contract Freeze
- `ExecutionStep` contract freeze and effect capture
- Determinism guard

---

## oris-evokernel [0.14.x] — EvoKernel Governance (2025 Q4–2026 Q1)

### Added
- Confidence lifecycle with revalidation and decay
- Risk-tiered policy engine
- Bounded autonomous intake with deduplication and prioritization
- Proposal-to-PR workflow with evidence-backed artifacts
- Governor-aware capture and solidification
- Approval checkpoints for sensitive work classes
- Promotion policy gating

---

## oris-evolution-network [0.5.0] — 2026 Q1

### Added
- OEN envelope with Ed25519 signing
- Gossip-based sync protocol with DNS and msgpack support
- mTLS option for network transport
- Rate limiter infrastructure

---

## oris-economics [0.2.0] — 2026 Q1

### Added
- Local EVU ledger with reputation accounting

---

*For full release notes including endpoint tables and test lists, see [RELEASE.md](RELEASE.md).*
