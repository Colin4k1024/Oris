# Oris Runtime — Consolidated Release History

This document merges all release notes from **v0.1.0** through **v0.21.0**. For the latest validation and links, see the end of this file.

---

## v0.1.0 — First crates.io release

First stable release of **Oris**: a programmable execution runtime for AI agents in Rust. Oris is a **runtime** for long-running, durable agent workflows: stateful graphs, checkpoints, interrupts, and recovery.

- **State graphs** — Define workflows as directed graphs; run, stream, and persist state (in-memory or SQLite).
- **Durable execution** — Checkpoint state, resume runs, and survive process restarts.
- **Human-in-the-loop** — Pause for approval or review, then resume with decisions.
- **Agents and tools** — Chat agents with tools; optional multi-agent and Deep Agent (planning, filesystem, skills).
- **RAG, chains, vector stores** — RAG, LLM chains, optional vector stores (PostgreSQL, Qdrant, SQLite, SurrealDB, etc.) behind features.

**Install:** `cargo add oris-runtime` (with vector store: `cargo add oris-runtime --features postgres`).

---

## v0.1.2 — PostgreSQL runtime parity fixes

- Fix PostgreSQL runtime store initialization so the runtime repository and shared Postgres stores can be constructed safely without panicking on missing Tokio context.
- Fix PostgreSQL schema version reads during runtime migration so startup, lease, dispatch, and contract tests succeed against the Postgres backend.

---

## v0.1.3 — PostgreSQL backup and restore runbook

- Add a PostgreSQL backup and restore runbook (backup, restore, validation queries, local rehearsal for runtime state).
- Add a repeatable rehearsal script that seeds a runtime schema, captures `pg_dump`, restores it, and verifies queued work plus lease ownership survive the round trip.

---

## v0.2.1 — Oris 2.0 Kernel: Interrupt Kernel

- **K3**: Interrupt struct — standardized representation (`id`, `thread_id`, `kind`, `payload_schema`, `created_at`, `step_id`) with `InterruptStore` trait and `InMemoryInterruptStore` implementation.

---

## v0.2.2 — Oris 2.0 Kernel: Execution Suspension State Machine

- **K3**: ExecutionSuspensionState — state transitions Running → Suspended → WaitingInput with safe worker teardown semantics.

---

## v0.2.3 — Oris 2.0 Kernel: Replay-Based Resume Semantics

- **K3**: ReplayResume — enforces Replay + Inject Decision semantics for idempotent resumes. ResumeDecision struct and ResumeResult with `events_replayed` and `idempotent` flag.

---

## v0.2.4 — Oris 2.0 Kernel: Unified Interrupt Routing

- **K3**: InterruptResolver trait with async `resolve(interrupt) -> Value`. UnifiedInterruptResolver routes UI, agents, policy engines, and API interrupts through source-specific handlers.

---

## v0.2.5 — Plugin Categories and Interfaces (K4)

- **K4**: PluginCategory enum (Node, Tool, Memory, LLMAdapter, Scheduler) for kernel plugin discovery and dispatch.
- Plugin interfaces: ToolPlugin, MemoryPlugin, LLMAdapter, SchedulerPlugin with `plugin_type()` and config-based factory methods. NodePlugin documented as Node category.
- New `plugins` module with PluginError and unit tests for PluginCategory.

---

## v0.2.6 — Plugin Determinism Declarations (K4)

- **K4**: PluginMetadata (deterministic, side_effects, replay_safe); PluginMetadata::conservative() and ::pure(); serde support.
- HasPluginMetadata trait; all plugin interfaces require it. Kernel enforcement helpers: allow_in_replay(meta), requires_sandbox(meta).

---

## v0.2.7 — Plugin Execution Sandbox (K4)

- **K4**: PluginExecutionMode enum (InProcess, IsolatedProcess, Remote). route_to_execution_mode(meta) selects mode from PluginMetadata; side effects → IsolatedProcess, pure → InProcess.

---

## v0.2.8 — Plugin Version Negotiation & Dynamic Registry (K4)

- **K4**: PluginCompatibility (plugin_api_version, kernel_compat, schema_hash); validate_plugin_compatibility for strict validation on load.
- NodePluginRegistry::unregister_plugin(plugin_type) for dynamic unloading and hot-loading.

---

## v0.2.9 — Finalize Lease-Based Execution (K5)

- **K5**: WorkerLease wrapping LeaseRecord for single-owner execution; verify_owner, is_expired, check_execution_allowed. Lease expiry and recovery in LeaseManager::tick.

---

## v0.2.10 — Zero-Data-Loss Failure Recovery Loop (K5)

- **K5**: CrashRecoveryPipeline and RecoveryStep (LeaseExpired → CheckpointReload → Replay → ReadyForDispatch). RecoveryContext with attempt_id and run_id. Integrates with LeaseManager::tick / RuntimeRepository::expire_leases_and_requeue.

---

## v0.2.11 — Context-Aware Scheduler Kernel (K5)

- **K5**: DispatchContext (optional tenant_id, priority, plugin_requirements, worker_capabilities). SkeletonScheduler::dispatch_one_with_context(worker_id, context) for future filtering/sorting.

---

## v0.2.12 — Safe Backpressure & Kernel Observability (K5)

- **K5**: RejectionReason enum (TenantLimit, CapacityLimit, Other). KernelObservability struct (optional reasoning_timeline, lease_graph, replay_cost, interrupt_latency_ms).

---

## v0.3.0 — EvoKernel Wave 0 Experimental Wiring

- Split EvoKernel support crates into stable `lib.rs` entrypoints with internal `core.rs` modules; preserve public re-export paths.
- Add full-evolution-experimental smoke test; keep runtime API contract and all-features Postgres build aligned for release validation.

---

## v0.4.0 — Spec-Aware EvoKernel Replay Selection

- Optional `spec_id` narrowing for EvoKernel replay selection; spec-linked mutations through evolution projection and exact-match replay.
- Repository layout: specs/behavior, specs/repair, specs/optimization, specs/evolution; execution server cancellation guard under all feature combinations.

---

## v0.5.0 — EvoKernel Local Economics Wiring

- EVU stake reservation for remote-facing asset export; insufficient local balance blocks publish without blocking local replay.
- Rewards or penalizes recorded remote publisher after replay; reputation as bounded secondary tie-breaker when replay candidates are otherwise equal.

---

## v0.5.1 — Replay Failure Revocation Fix

- Record replay validation failures in evolution event log; route updated failure count through governor policy.
- Auto-revoke and quarantine promoted assets after configured replay failure threshold; revoked assets drop out of replay selection.

---

## v0.6.0 — Environment-Aware Replay Ranking

- Selector scoring weights environment similarity; replay candidates ranked by recorded execution environment match.
- Replay prefers closest matching Capsule within matching assets for better reuse when multiple proven solutions share signals.

---

## v0.7.0 — EvoKernel Observability Metrics

- Execution server exposes store-derived evolution metrics (replay success, promotion ratio, revoke frequency, mutation velocity) on Prometheus `/metrics`.
- Built-in `/healthz` endpoint for evolution observability snapshot (scrape and readiness).

---

## v0.8.0 — Kernel Trace Context and Observability

- Execution server derives KernelObservability from checkpoint history and active lease context (no placeholder-only telemetry).
- Job timeline and timeline export APIs include optional trace context for correlating timeline reads with runtime spans.

---

## v0.8.1 — Deterministic checkpoint recovery hardening

- Kernel runs resume from latest saved checkpoint; interrupts/completions persist current snapshot before returning.
- Replay and execution-log reconstruction propagate snapshot store failures instead of silent fallback for deterministic replay verification.

---

## v0.8.2 — DEVLOOP proposal example wiring

- `examples/evo_oris_repo` uses oris-runtime re-exports with full-evolution-experimental; runs AgentTask → MutationProposal → capture_from_proposal → replay_or_fallback.
- Example shows two agent sources in same capture pipeline and replay on second pass; docs and feature-wiring coverage updated.

---

## v0.9.0 — OEN quarantine release path

- Remote OEN imports keep capsules quarantined until local replay validation succeeds; no immediate promotion from remote lifecycle.
- Experimental OEN replay path can cold-start from quarantined remote capsules and promotes only after first successful local replay validation.

---

## v0.9.1 — EvoKernel regression suite expansion

- External regression coverage for replay determinism, sandbox boundary enforcement, governor blast-radius gating, replay-failure revocation.
- End-to-end replay lifecycle path kept in same external suite for full capture-to-reuse integration test.

---

## v0.10.0 — Governor rate limits and confidence decay

- Time-window mutation rate limits and retry cooldown in EvoKernel governor; rapid successive mutations can be deferred.
- Confidence decay and confidence-history-based regression revocation; new regression coverage in oris-governor and EvoKernel black-box tests.

---

## v0.11.0 — EvoKernel signal extraction and solidification queries

- Deterministic EvoKernel signal extraction inputs/outputs; persisted SignalsExtracted evolution event for normalized signal set and hash.
- Direct EvoKernel::select_candidates(...) query path; expanded regression coverage for signal stability and local candidate lookup.

---

## v0.12.0 — EvoKernel multi-agent coordination

- Multi-agent coordination DTOs on agent contract: roles, coordination primitives, tasks, messages, plans, results.
- MultiAgentCoordinator and EvoKernel::coordinate(...) with deterministic sequential, parallel, conditional scheduling and retry-aware failure handling.
- Regression coverage for planner-to-coder handoffs, repair-after-failure, optimizer gating, parallel merge ordering, retries, conditional skips.

---

## v0.13.0 — EvoKernel bootstrap and initial seeding

- SeedTemplate, BootstrapReport, EvoKernel::bootstrap_if_empty(...) for opt-in initial seeding of empty evolution stores.
- Built-in four-template bootstrap catalog; append-only seed events, deterministic IDs, quarantined seed capsules until local validation.
- Regression coverage for bootstrap counts, quarantine state, idempotence, append-only history, seed discoverability via select_candidates(...).

---

## v0.13.1 — EvoKernel lifecycle replay fixes

- Remote Evo asset sharing: exported/fetched promoted assets include mutation payload required for first local replay.
- Replay compatibility: legacy ReplayExecutor entrypoint restored; replay execution IDs recorded separately in Evo events.

---

## v0.13.2 — Remote replay follow-up fixes

- Normalized remote cold-start replay scoring so overlapping signal fragments do not inflate candidate scores above full query coverage.
- Evo asset export/fetch overhead reduced by reusing single event scan for projection rebuild and replay payload packaging.

---

## v0.13.3 — Evo consistency hardening

- Remote replay publisher attribution: reputation bias and EVU settlement follow the capsule actually selected for replay when multiple remote capsules share a gene.
- Evo projection reads and remote import: selector, replay, fetch, metrics, repeated remote syncs observe same store snapshot contract without duplicate downgrade writes.

---

## v0.14.0 — EvoKernel staged self-evolution hardening

- Deterministic task-class replay matching; strengthened negative controls for unrelated task classes.
- Continuous confidence lifecycle (confidence decay, revalidation-driven replay eligibility).
- Agent-facing replay feedback: planner directives, fallback reasons, reasoning-avoidance metrics.
- Bounded supervised DEVLOOP policy coverage and staged self-evolution acceptance checklist updates.

---

## v0.15.0 — Add EvoMap `/a2a/*` Compatibility Facade Routes

- Routes `/a2a/hello`, `/a2a/tasks/distribute`, `/a2a/tasks/claim`, `/a2a/tasks/report` mapping to existing A2A compatibility handlers.
- Route-contract regression coverage; feature-gate so `/a2a/*` routes unavailable when evolution-network-experimental is disabled.

---

## v0.16.0 — Remote issue orchestration and roadmap sync

- Remote GitHub issue listing and deterministic issue selection in oris-orchestrator (P0 > P1, then milestone, then issue number); RFC/blocked filtering and single-issue execution entrypoints.
- Roadmap sync backfill by exact title for empty issue_number; --track scoping and ambiguity-safe skip; CSV bookkeeping reconciled before orchestrator selection.
- Maintainer workflow docs: "sync roadmap first, then select issue" release loop.

---

## v0.17.0 — Harden `/a2a/fetch` validation determinism

- Deterministic a2a_error_code=ValidationFailed details for gep-a2a message_type mismatches in /a2a/fetch compatibility parsing.
- Regression coverage for error code and expected/actual message-type payload for invalid fetch envelope requests.

---

## v0.17.1 — RFC roadmap closeout alignment

- RFC closeout decisions in docs/ORIS_2.0_STRATEGY.md for #106–#109: delivered /a2a outcomes and deferred themes (deterministic scheduler hardening, MCP, long-horizon ecosystem).
- Issue/release state synchronized so roadmap RFC closure ties to a published runtime version.

---

## v0.18.0 — Deterministic scheduler lease hardening for A2A flows

- Active-lease ownership checks for /a2a/tasks/report and /a2a/task/complete: report/complete require active claim by caller; stale/expired leases and non-owner writes rejected.
- SQLite lease touch: heartbeat-style extension only for active owner.
- Deterministic ordering tiebreaker (session_id) for equal enqueue timestamps when listing/claiming compatibility tasks.
- Regression: report without active claim, complete with expired lease, claim ordering with enqueue-time ties, owner-only lease touch.

---

## v0.18.1 — Deterministic scheduler regression matrix for A2A lease and replay parity

- Deterministic matrix test for /a2a: claim conflict under active lease, heartbeat visibility before/after forced lease expiry, reclaim after expiry, completion idempotency on duplicate complete.
- Explicit non-owner running-report rejection (sender-scoped 404). Operator docs: single command for matrix and triage map for matrix-failure signals.

---

## v0.19.0 — MCP bootstrap and capability discovery scaffold

- mcp-experimental feature: MCP bootstrap config (ORIS_MCP_BOOTSTRAP_ENABLED, transport, server metadata) and startup wiring.
- Endpoints /v1/mcp/bootstrap, /v1/mcp/capabilities; default capability registry mapping (oris.runtime.jobs.run → POST /v1/jobs/run); disabled-by-default.
- Runtime tests and starter-axum docs/smoke for MCP bootstrap and capability discovery.

---

## v0.20.0 — Stable /a2a production boundary

- a2a-production feature: stable /a2a/* compatibility routes for production.
- Evolution-network publish/fetch/revoke and legacy /evolution/a2a/* remain behind experimental gates unless enabled.
- Route-boundary regression and migration/runbook docs for stable vs experimental runtime behavior.

---

## v0.21.0 — EvoMap Feature Expansion

- **RuntimeRepository**: Recipe and Organism CRUD in SQLite; PostgreSQL RuntimeRepository with Worker registration, Dispute management, Recipe, Organism, Session persistence. API handlers (evomap_recipe_create, evomap_recipe_get, evomap_recipe_fork, evomap_organism_express, evomap_organism_get) use RuntimeRepository.
- **Peer Discovery & Gossip**: gossip module in oris-evolution-network — PeerRegistry, PeerConfig, GossipMessage/GossipKind, GossipBuilder.
- **Automatic Publishing Gate**: publish_gate in oris-orchestrator — PublishGate, PublishGateConfig, PublishTarget, exponential backoff retry, publish status tracking.
- **Evolver Automation**: evolver in oris-evolution — EvolutionSignal, SignalType, MutationProposal, MutationRiskLevel, EvolverConfig, EvolverAutomation, SignalBuilder, ValidationResult.
- **E2E**: recipe_crud_lifecycle, organism_lifecycle, recipe_fork_lifecycle; SQLite tests 44 (from 41).

---

## Validation (current)

- `cargo fmt --all -- --check`
- `cargo build --all --release --all-features`
- `cargo test --release --all-features`

## Links

- **Crate:** [crates.io/crates/oris-runtime](https://crates.io/crates/oris-runtime)
- **Docs:** [docs.rs/oris-runtime](https://docs.rs/oris-runtime)
- **Repo:** [github.com/Colin4k1024/Oris](https://github.com/Colin4k1024/Oris)
- **Examples:** [examples](https://github.com/Colin4k1024/Oris/tree/main/examples)
