# Oris Runtime — Consolidated Release History

## Current Capability Note

As of March 15, 2026, the most accurate product statement for the checked-in self-evolution surface is:

> Oris supports a supervised closed-loop self-evolution path with bounded acceptance gating.

The repository currently supports replay-driven learning, auditable mutation proposal contracts, replay-assisted supervised execution with fail-closed safety, and bounded branch or pull-request artifact preparation. It does not yet claim a fully autonomous software-improvement loop that independently discovers issues, merges code, publishes packages, or performs releases.

See [docs/evokernel/current-project-status.md](docs/evokernel/current-project-status.md) for the full current boundary statement.

This document merges all release notes from **v0.1.0** through **v0.39.0**.

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

## v0.22.0 — Runtime Signal Extraction

Add automatic runtime signal extraction as a dedicated stage in the evolution loop for self-evolving agents.

### What's in this release

- **Runtime Signal Extraction**: New `RuntimeSignalExtractor` module in oris-evokernel for automatic signal extraction from execution context.
- `CompilerDiagnosticsParser`: extract signals from rustc errors and warnings.
- `StackTraceParser`: extract signals from panic stack traces.
- `LogAnalyzer`: extract signals from execution logs such as timeouts, resource exhaustion, and test failures.
- Signal types: `CompilerDiagnostic`, `RuntimePanic`, `Timeout`, `TestFailure`, `PerformanceIssue`, `ResourceExhaustion`, `ConfigError`, `SecurityIssue`, `GenericError`.
- Signals are deterministically extracted using regex pattern matching.

### Validation

- `cargo fmt --all`
- `cargo test -p oris-evokernel` (30 tests passed)
- `cargo build -p oris-runtime --all-features`
- `cargo publish -p oris-runtime --all-features --dry-run`
- `cargo publish -p oris-runtime --all-features`

### Links

- Crate: https://crates.io/crates/oris-runtime
- Docs: https://docs.rs/oris-runtime
- Repo: https://github.com/Colin4k1024/Oris

---

## v0.22.1 — GEP Compatibility Matrix Hardening

Strengthen GEP envelope/schema compatibility validation so protocol mismatch and payload errors return deterministic A2A-compatible error details.

### What's in this release

- Added deterministic `a2a_error_code` details for GEP envelope and hello parsing failures, including protocol, version, message type, sender, and payload evidence.
- Expanded GEP compliance tests to lock schema, version, envelope, `message_type`, and fallback translation behavior against regressions.

### Validation

- `cargo fmt --all -- --check`
- `cargo test -p oris-runtime --features "full-evolution-experimental execution-server sqlite-persistence" execution_server::api_handlers::tests:: -- --nocapture`
- `cargo test -p oris-evolution --lib`
- `cargo build --verbose --all --release --all-features`
- `cargo test --release --all-features`
- `cargo publish -p oris-execution-runtime --dry-run --registry crates-io`
- `cargo publish -p oris-execution-runtime --registry crates-io`
- `cargo publish -p oris-runtime --all-features --dry-run --registry crates-io`
- `cargo publish -p oris-runtime --all-features --registry crates-io`

### Links

- Crate: https://crates.io/crates/oris-runtime
- Docs: https://docs.rs/oris-runtime
- Repo: https://github.com/Colin4k1024/Oris

---

## v0.23.0 — GEP Delta Sync and Resume Token

Add incremental GEP synchronization primitives so peers can pull deltas with resumable cursors and receive machine-readable sync audit evidence.

### What's in this release

- Added `since_cursor` and `resume_token` support for publish and fetch protocol messages, with deterministic cursor progression and resume token validation.
- Added `sync_audit` response evidence with scanned, applied, skipped, and failed counts plus reasons, and idempotent import behavior across evokernel and runtime compatibility APIs.
- Extended runtime A2A fetch compatibility APIs and tests to verify delta synchronization and resume-token continuation end-to-end.

### Validation

- `cargo fmt --all -- --check`
- `cargo test -p oris-evolution-network`
- `cargo test -p oris-evokernel`
- `cargo test -p oris-runtime evolution_a2a_fetch_returns_sync_cursor_and_supports_resume_token_delta --features "sqlite-persistence,execution-server,agent-contract-experimental,evolution-network-experimental" -- --nocapture --test-threads=1`
- `cargo test --workspace -- --skip official_experience_reuse_with_real_qwen`
- `cargo build --verbose --all --release --all-features`
- `cargo test --release --all-features -- --skip official_experience_reuse_with_real_qwen`
- `cargo publish -p oris-evolution --registry crates-io`
- `cargo publish -p oris-governor --registry crates-io`
- `cargo publish -p oris-sandbox --registry crates-io`
- `cargo publish -p oris-spec --registry crates-io`
- `cargo publish -p oris-evolution-network --registry crates-io`
- `cargo publish -p oris-evokernel --registry crates-io`
- `cargo publish -p oris-runtime --all-features --dry-run --registry crates-io`
- `cargo publish -p oris-runtime --all-features --registry crates-io`

### Links

- Crate: https://crates.io/crates/oris-runtime
- Docs: https://docs.rs/oris-runtime
- Repo: https://github.com/Colin4k1024/Oris

---

## v0.24.0 — Stable Task-Class Replay Evidence

Strengthen self-evolution replay generalization so semantically equivalent multi-signal tasks reuse learned capsules more reliably while preserving stable machine-readable task-class evidence.

### What's in this release

- Normalized semantically equivalent task signals across evolution and evokernel replay matching, including missing-state aliases and filler-token suppression, so same-class tasks replay consistently without regressing adjacent negative samples.
- Stabilized task-class evidence in replay feedback and derived gene metadata, and added regression coverage to prove multi-signal semantic variants keep replay labels audit-friendly and deterministic.

### Validation

- `cargo fmt --all -- --check`
- `cargo test -p oris-evolution normalized_signal_overlap -- --nocapture`
- `cargo test -p oris-evokernel --test evolution_lifecycle_regression multi_signal_semantic_variants_keep_task_class_feedback_stable -- --nocapture`
- `cargo test -p oris-evokernel --test evolution_lifecycle_regression replay_feedback_surfaces_planner_hints_and_reasoning_savings -- --nocapture`
- `cargo test -p oris-evokernel --lib replay_roi_release_gate_summary_window_boundary_filters_old_events -- --nocapture`
- `cargo test -p oris-evokernel --release --lib replay_roi_release_gate_summary_window_boundary_filters_old_events -- --nocapture`
- `cargo test -p oris-evokernel --test evolution_lifecycle_regression -- --nocapture`
- `cargo test -p oris-runtime --test agent_self_evolution_travel_network --features full-evolution-experimental travel_network_demo_flow_captures_publishes_imports_and_replays -- --nocapture`
- `cargo test -p oris-runtime --release --test agent_self_evolution_travel_network --features full-evolution-experimental travel_network_demo_flow_captures_publishes_imports_and_replays -- --nocapture`
- `cargo test --workspace`
- `cargo build --verbose --all --release --all-features`
- `cargo test --release --all-features`
- `cargo publish -p oris-agent-contract --dry-run --allow-dirty --registry crates-io`
- `cargo publish -p oris-agent-contract --allow-dirty --registry crates-io`
- `cargo publish -p oris-evolution --dry-run --allow-dirty --registry crates-io`
- `cargo publish -p oris-evolution --allow-dirty --registry crates-io`
- `cargo publish -p oris-evokernel --dry-run --allow-dirty --registry crates-io`
- `cargo publish -p oris-evokernel --allow-dirty --registry crates-io`
- `cargo publish -p oris-runtime --all-features --dry-run --allow-dirty --registry crates-io`
- `cargo publish -p oris-runtime --all-features --allow-dirty --registry crates-io`

### Links

- Crate: https://crates.io/crates/oris-runtime
- Docs: https://docs.rs/oris-runtime
- Repo: https://github.com/Colin4k1024/Oris

---

## v0.25.0 — Continuous Confidence Control

Harden continuous confidence control so stale or regressing self-evolution assets emit deterministic reason codes, carry auditable evidence summaries, and stay aligned through the runtime evolution facade.

### What's in this release

- Unified confidence transition evidence generation for replay-failure revocation and governor-driven confidence regression demotion, including decayed confidence, decay ratio, and phase-tagged summaries.
- Added regression assertions for stale confidence revalidation and local governor revocation so downgrade paths prove the emitted evidence contract instead of only checking terminal state.
- Exposed `TransitionEvidence` and `TransitionReasonCode` through the runtime evolution facade and locked that surface with feature wiring coverage.

### Validation

- `cargo fmt --all -- --check`
- `cargo test -p oris-evokernel --test evolution_lifecycle_regression local_capture_uses_existing_confidence_context_for_governor -- --nocapture`
- `cargo test -p oris-evokernel --test evolution_lifecycle_regression stale_confidence_forces_revalidation_before_replay -- --nocapture`
- `cargo test -p oris-evokernel --lib`
- `cargo test -p oris-runtime --test evolution_feature_wiring --features full-evolution-experimental -- --nocapture`
- `cargo test --workspace`
- `cargo build --verbose --all --release --all-features`
- `cargo test --release --all-features`
- `cargo publish -p oris-evokernel --registry crates-io`

### Links

- Crate: https://crates.io/crates/oris-runtime
- Docs: https://docs.rs/oris-runtime
- Repo: https://github.com/Colin4k1024/Oris

---

## v0.26.0 — Replay ROI Stability

Stabilize replay ROI metrics so runtime release-gate evidence stays comparable to metrics snapshots across the same replay history.

### What's in this release

- Unified evokernel replay ROI aggregation so `metrics_snapshot()` and replay release-gate summaries consume the same task-class and source totals.
- Preserved legacy fallback reconstruction for histories that predate `ReplayEconomicsRecorded`, preventing release-gate summaries from drifting to zero while metrics still report replay activity.
- Tightened runtime travel-network regression coverage so release-gate contract input must match the generated replay ROI summary for the same window.

### Validation

- `cargo fmt --all -- --check`
- `cargo test -p oris-evokernel --lib replay_roi_release_gate_summary_ -- --nocapture`
- `cargo test -p oris-runtime --test agent_self_evolution_travel_network --features full-evolution-experimental -- --nocapture`
- `cargo test --workspace`
- `cargo build --verbose --all --release --all-features`
- `cargo test --release --all-features`
- `cargo publish -p oris-evokernel --registry crates-io`
- `cargo publish -p oris-runtime --all-features --dry-run --registry crates-io`
- `cargo publish -p oris-runtime --all-features --registry crates-io`

### Links

- Crate: https://crates.io/crates/oris-runtime
- Docs: https://docs.rs/oris-runtime
- Repo: https://github.com/Colin4k1024/Oris

---

## v0.27.0 — Bounded Supervised Devloop Expansion

`oris-runtime` now exposes a bounded supervised DEVLOOP path for small multi-file docs workflows while keeping failure handling fail-closed and auditable.

### What's in this release

- Expand supervised DEVLOOP from single-file docs tasks to bounded multi-file docs tasks under `docs/` with deterministic file-count limits.
- Keep `reason_code`, `recovery_hint`, and fail-closed rejection semantics aligned across API outcomes, evolution events, and runtime facade coverage.
- Update devloop documentation to reflect the new bounded docs-task surface.

### Validation

- `cargo fmt --all -- --check`
- `cargo test -p oris-evokernel --test evolution_lifecycle_regression supervised_devloop_ -- --nocapture`
- `cargo test -p oris-runtime --test evolution_feature_wiring --features full-evolution-experimental -- --nocapture`
- `cargo test --workspace`
- `cargo build --verbose --all --release --all-features`
- `cargo test --release --all-features`
- `cargo publish -p oris-runtime --all-features --dry-run`
- `cargo publish -p oris-runtime --all-features`

### Links

- Crate: https://crates.io/crates/oris-runtime
- Docs: https://docs.rs/oris-runtime
- Repo: https://github.com/fanjia1024/oris

---

## v0.28.0 — Federated Revocation Hardening

`oris-runtime` now fail-closes spoofed remote revoke requests and preserves remote attribution through replay revocation evidence.

### What's in this release

- Hardened federated revoke handling so imported remote assets can only be revoked by the sender that originally published them, while mixed-ownership revoke requests are rejected as a whole.
- Added stable remote attribution evidence for replay-failure revocations and locked the import, replay, and revoke path with evokernel and travel-network regressions.

### Validation

- `cargo fmt --all -- --check`
- `cargo test -p oris-evokernel --test evolution_lifecycle_regression remote_revoke_ -- --nocapture`
- `cargo test -p oris-evokernel --test evolution_lifecycle_regression remote_replay_failure_ -- --nocapture`
- `cargo test -p oris-evokernel --lib`
- `cargo test -p oris-runtime --test agent_self_evolution_travel_network --features full-evolution-experimental -- --nocapture`
- `cargo test --workspace`
- `cargo build --verbose --all --release --all-features`
- `cargo test --release --all-features`
- `cargo publish -p oris-evokernel --dry-run --registry crates-io`
- `cargo publish -p oris-runtime --all-features --dry-run --registry crates-io`
- `cargo publish -p oris-runtime --all-features --registry crates-io`

### Links

- Crate: https://crates.io/crates/oris-runtime
- Docs: https://docs.rs/oris-runtime
- Repo: https://github.com/fanjia1024/oris

---

## v0.29.0 — Self-Evolution Candidate Intake Contracts

`oris-runtime` now exposes a bounded GitHub issue-style self-evolution candidate intake path with machine-readable accept/reject decisions and fail-closed reason codes.

Also shipped:

- `oris-agent-contract v0.4.0`
- `oris-evokernel v0.11.0`

### What's in this release

- Added `SelfEvolutionCandidateIntakeRequest`, `SelfEvolutionSelectionReasonCode`, and `SelfEvolutionSelectionDecision` to the public agent contract surface.
- Added `EvoKernel::select_self_evolution_candidate(...)` so bounded GitHub issue-shaped candidates can be accepted or rejected before proposal generation.
- Locked accept, reject, and fail-closed selection behavior with evokernel regressions and runtime facade wiring coverage.

### Validation

- `cargo fmt --all -- --check`
- `cargo test -p oris-evokernel --test evolution_lifecycle_regression candidate_intake_ -- --nocapture`
- `cargo test -p oris-runtime --test evolution_feature_wiring --features full-evolution-experimental -- --nocapture`
- `cargo test --workspace`
- `cargo build --verbose --all --release --all-features`
- `cargo test --release --all-features`

### Links

- Crate: https://crates.io/crates/oris-runtime
- Docs: https://docs.rs/oris-runtime
- Repo: https://github.com/Colin4k1024/Oris

---

## v0.30.0 — Structured Mutation Proposal Contracts

`oris-runtime` now exposes structured self-evolution mutation proposal contracts that declare bounded scope, validation budget, approval requirements, expected evidence, and fail-closed rejection semantics before execution begins.

### What's in this release

- Added machine-readable self-evolution mutation proposal contracts to the experimental agent contract surface, including `proposal_scope`, `validation_budget`, `approval_required`, `expected_evidence`, `reason_code`, and `fail_closed`.
- Added `EvoKernel::prepare_self_evolution_mutation_proposal(...)` and pre-execution proposal validation so malformed or out-of-bounds supervised mutations are rejected before execution starts.
- Extended evokernel regression coverage and runtime feature wiring coverage for accepted proposal generation, fail-closed scope rejection, and missing target-file rejection.

### Validation

- `cargo fmt --all -- --check`
- `cargo test -p oris-evokernel --test evolution_lifecycle_regression mutation_proposal_ -- --nocapture`
- `cargo test -p oris-runtime --test evolution_feature_wiring --features full-evolution-experimental -- --nocapture`
- `cargo test --workspace`
- `cargo build --verbose --all --release --all-features`
- `cargo test --release --all-features`
- `cargo publish -p oris-runtime --all-features --dry-run --registry crates-io --allow-dirty`
- `cargo publish -p oris-runtime --all-features --registry crates-io --allow-dirty`

### Links

- Crate: https://crates.io/crates/oris-runtime
- Docs: https://docs.rs/oris-runtime
- Repo: https://github.com/Colin4k1024/Oris

---

## Common Validation Baseline

- `cargo fmt --all -- --check`
- `cargo build --all --release --all-features`
- `cargo test --release --all-features`

## Repository Links

- **Crate:** [crates.io/crates/oris-runtime](https://crates.io/crates/oris-runtime)
- **Docs:** [docs.rs/oris-runtime](https://docs.rs/oris-runtime)
- **Repo:** [github.com/Colin4k1024/Oris](https://github.com/Colin4k1024/Oris)
- **Examples:** [examples](https://github.com/Colin4k1024/Oris/tree/main/examples)


---

# oris-intake v0.2.0 - CI/CD Webhook Integration

GitHub CI failure events now flow automatically into the Oris intake pipeline via `GithubIntakeSource`, enabling zero-touch Detect for `check_run` and `workflow_run` webhook events.

## What's in this release

- **`GithubIntakeSource`** — concrete `IntakeSource` implementation that processes raw GitHub webhook JSON for both `check_run` (CI check suite failures) and `workflow_run` events; auto-dispatches on payload shape when no explicit event type is supplied
- **`GithubCheckRunEvent`** — typed deserialization struct for GitHub `check_run` webhook payloads including `id`, `name`, `head_sha`, `conclusion`, and `output` (title + summary)
- **`from_github_check_run()`** — converts a parsed `GithubCheckRunEvent` into an `IntakeEvent` with severity, signals (`check_run_conclusion:*`, `commit_sha:*`, `output_title:*`), and timestamp
- Signal dedup ≥ 95% — same-root-cause events are deduplicated; existing `Deduplicator` validated to 19/20 (95%) hit rate in unit test
- Priority label ordering validated: Critical > High > Medium > Low > Info; Critical scores ≥ 75
- 8 new integration tests in `crates/oris-intake/tests/webhook_integration.rs` covering all 4 acceptance criteria

## Validation

- `cargo fmt --all -- --check` ✓
- `cargo test -p oris-intake` — 25/25 pass (17 unit + 8 integration tests)

## Links

- Crate: https://crates.io/crates/oris-intake
- Repo: https://github.com/Colin4k1024/Oris


---

# v0.31.0 - Replay-Assisted Supervised Execution Loop

`oris-runtime` now exposes a replay-aware supervised execution contract that unifies execution decision, replay outcome, fallback reason, validation status, evidence summary, and fail-closed recovery hints in one machine-readable result.

## What's in this release

- Extended `SupervisedDevloopOutcome` with unified replay-assisted execution fields, including `execution_decision`, `replay_outcome`, `fallback_reason`, `validation_outcome`, `evidence_summary`, `reason_code`, and `recovery_hint`.
- Connected `EvoKernel::run_supervised_devloop(...)` to the replay path so approved proposals can reuse existing capsules, fall back to bounded execution when replay misses safely, and fail closed when replay validation or patch reuse is unsafe.
- Added evokernel regression coverage for replay-hit reuse and replay-validation fail-closed behavior, and updated runtime wiring coverage for the new contract surface.

## Validation

- cargo fmt --all -- --check
- cargo test -p oris-evokernel --test evolution_lifecycle_regression replay_supervised_execution_ -- --nocapture
- cargo test -p oris-runtime --test evolution_feature_wiring --features full-evolution-experimental -- --nocapture
- cargo test -p oris-runtime --test agent_self_evolution_travel_network --features full-evolution-experimental -- --nocapture
- cargo test --workspace
- cargo build --verbose --all --release --all-features
- cargo test --release --all-features
- cargo publish -p oris-runtime --all-features --dry-run --registry crates-io --allow-dirty
- cargo publish -p oris-runtime --all-features --registry crates-io --allow-dirty

## Links

- Crate: https://crates.io/crates/oris-runtime
- Docs: https://docs.rs/oris-runtime
- Repo: https://github.com/Colin4k1024/Oris


---

# v0.32.0 - Pipeline Detect/Execute Stage Integration with SignalExtractor and Sandbox

`oris-runtime` now wires the evolution pipeline's Detect stage to `SignalExtractorPort` for runtime signal ingestion (compiler errors, panics, test failures) and the Execute stage to `SandboxPort` for safe sandboxed mutation execution, with per-stage wall-clock timing written to `PipelineContext.stage_timings`.

## What's in this release

- Added `SignalExtractorPort` and `SandboxPort` traits in `oris-evolution::port`, enabling injection of real signal extraction and sandbox execution into `StandardEvolutionPipeline` without creating circular dependencies.
- Detect stage: when a `SignalExtractorPort` is injected, `PipelineContext.signals` is populated from runtime diagnostics (compiler output, stack traces, execution logs). Falls back to pass-through when no extractor is provided (backward-compatible).
- Execute stage: when a `SandboxPort` is injected, the first `MutationProposal` is applied via the sandbox and the result is recorded as `PipelineContext.execution_result`. Falls back to synthetic stub when no sandbox is provided.
- All 8 pipeline stages now record wall-clock duration in `PipelineContext.stage_timings: HashMap<String, Duration>`, replacing the always-`None` `duration_ms` fields.
- Added `RuntimeSignalExtractorAdapter` and `LocalSandboxAdapter` in `oris-evokernel::adapters`, providing ready-to-use implementations of the new port traits backed by the existing `RuntimeSignalExtractor` and `LocalProcessSandbox`.
- Added `PipelineContext.extractor_input: Option<SignalExtractorInput>` carrying raw compiler/trace/log text into the Detect stage.
- Added `StandardEvolutionPipeline::with_signal_extractor()` and `with_sandbox()` builder methods for port injection.

## Validation

- cargo fmt --all -- --check
- cargo test -p oris-evolution
- cargo test -p oris-runtime --test evolution_feature_wiring --features full-evolution-experimental
- cargo build --verbose --all --release --all-features
- cargo test --release --all-features
- cargo publish -p oris-runtime --all-features --dry-run --registry crates-io --allow-dirty
- cargo publish -p oris-runtime --all-features --registry crates-io --allow-dirty

## Links

- Crate: https://crates.io/crates/oris-runtime
- Docs: https://docs.rs/oris-runtime
- Repo: https://github.com/Colin4k1024/Oris


---

# Release v0.32.1

## Issue #243 — GeneStore SQLite CRUD + Solidify/Reuse 集成

### 变更摘要

实现了 `oris-genestore` SQLite 持久化层与进化 pipeline 的完整集成，
将 Solidify/Reuse 阶段从 placeholder 升级为真正的持久化路径。

### 新增内容

#### `oris-evolution`
- `port.rs`：新增 `GeneStorePersistPort` trait（trait-port 注入模式）
  - `persist_gene(gene_id, signals, strategy, validation) -> bool`
  - `mark_reused(gene_id, capsule_ids) -> bool`
- `pipeline.rs`：Solidify/Reuse 阶段接线
  - `StandardEvolutionPipeline::with_gene_store()` 构建器方法
  - Solidify 阶段：为每个候选基因调用 `GeneStorePersistPort::persist_gene`
  - Reuse 阶段：为每个候选基因调用 `GeneStorePersistPort::mark_reused`
  - `execute_stage` 同步路径同样接线
- 新增 `test_solidify_reuse_calls_gene_store` 集成测试（MockGeneStore）

#### `oris-genestore`
- `store.rs`：13 项单元测试覆盖全部 CRUD 路径
  - Gene upsert/get/delete、search_genes（tag 索引）、decay_all
  - record_gene_outcome（成功/失败）、stale_genes
  - Capsule CRUD + record_capsule_outcome
- `migrate.rs`：新增 JSONL → SQLite 一次性迁移工具
  - `migrate::from_jsonl(path, store) -> Result<usize>`
  - 跳过空行、`#` 注释行、JSON 解析失败行（带警告）
  - 2 项测试：roundtrip 迁移、无效行跳过

#### `oris-evokernel`
- `adapters.rs`：新增 `SqliteGeneStorePersistAdapter`
  - 实现 `GeneStorePersistPort`，将 oris-evolution Gene 映射到 oris-genestore Gene
  - 异步桥接模式与 `LocalSandboxAdapter` 一致
- `Cargo.toml`：添加 `oris-genestore = "0.1.0"` 和 `anyhow = "1.0"` 依赖

#### `oris-runtime`（集成测试）
- `evolution_feature_wiring.rs`：新增 `genestore_persist_adapter_resolves` 测试
  - 验证 `oris_runtime::evolution::adapters::SqliteGeneStorePersistAdapter` 可达
  - 验证 `:memory:` 内存数据库可正常打开

### 验收标准核对

- ✅ Gene 写入和读取通过 SQLite 持久化（13 个单元测试）
- ✅ 查询复杂度 O(log n) / O(1)（SQLite 索引：idx_gene_tags_tag、idx_genes_confidence）
- ✅ 现有 JSONL gene 可一次性迁移（`migrate::from_jsonl`）
- ✅ 与 oris-evolution Solidify/Reuse 路径集成测试通过（evolution_feature_wiring 2 passed）

### 验证

```
cargo fmt --all -- --check        ✅
cargo test -p oris-genestore      13 passed
cargo test -p oris-evolution      54 passed
cargo test --test evolution_feature_wiring --features full-evolution-experimental  2 passed
cargo build --all --release --all-features  ✅
cargo test --release --all-features  0 failures
```


---

# Release Notes — oris-runtime v0.33.0

## Summary

Implements **EVO26-AUTO-01**: Autonomous Candidate Intake From CI and Runtime Signals.

This release adds the first machine-readable contract for discovering evolution
candidates autonomously from CI and runtime diagnostic signals, without requiring
a caller-supplied GitHub issue number.

## Changes

### New: Autonomous Intake Contract (`oris-agent-contract`)

Five new public types:

| Type | Purpose |
|------|---------|
| `AutonomousCandidateSource` | Signal source classification (`CiFailure`, `TestRegression`, `CompileRegression`, `LintRegression`, `RuntimeIncident`) |
| `AutonomousIntakeReasonCode` | Outcome reason code (`Accepted`, `UnsupportedSignalClass`, `AmbiguousSignal`, `DuplicateCandidate`, `UnknownFailClosed`) |
| `DiscoveredCandidate` | A single discovered candidate with `dedupe_key`, `candidate_source`, `candidate_class`, `signals`, `reason_code`, `fail_closed` |
| `AutonomousIntakeInput` | Input to the intake method: `source_id`, `candidate_source`, `raw_signals` |
| `AutonomousIntakeOutput` | Batch output: `candidates`, `accepted_count`, `denied_count` |

Two new helper constructors:

- `accept_discovered_candidate(dedupe_key, source, class, signals, summary)` → `DiscoveredCandidate`
- `deny_discovered_candidate(dedupe_key, source, signals, reason_code)` → `DiscoveredCandidate`

### New: `EvoKernel::discover_autonomous_candidates` (`oris-evokernel`)

New method on `EvoKernel<S>`:

```rust
pub fn discover_autonomous_candidates(
    &self,
    input: &AutonomousIntakeInput,
) -> AutonomousIntakeOutput
```

Behavior:
- Normalises raw signals (trim, lowercase, sort, dedup)
- Computes a stable `dedupe_key` via `stable_hash_json`
- Checks for duplicate candidates in the evolution store (via `SignalsExtracted` events)
- Maps signal source to `BoundedTaskClass` (`LintFix` for compile/test/lint/CI; `None` for `RuntimeIncident`)
- Fail-closes on empty signals, ambiguous sources, or unknown outcomes
- Does **not** generate mutation proposals, trigger task planning, or run executors

### Validation

- `cargo test -p oris-evokernel --test evolution_lifecycle_regression autonomous_intake_` — 5 new tests, all passing
- `cargo test -p oris-runtime --test evolution_feature_wiring --features full-evolution-experimental` — 4 tests, all passing
- `cargo build --all --release --all-features` — clean build
- `cargo test --release --all-features` — no failures

## Non-Goals (not included in this release)

- Mutation proposal generation from discovered candidates
- Autonomous execution or task planning
- PR/release automation triggered by intake
- Recovery or incident escalation beyond fail-closed signaling

## Upgrade Notes

No breaking changes. New types are additive. Existing `select_self_evolution_candidate` and `prepare_self_evolution_mutation_proposal` are unchanged.


---

# Release Notes — oris-runtime v0.34.0

## Summary

Implements **EVO26-AUTO-02: Bounded Task Planning and Risk Scoring For Autonomous Intake** (Issue #265).

Adds a machine-readable planning contract that scores every `DiscoveredCandidate` produced by
AUTO-01 intake before any mutation is proposed. The planner assigns a risk tier, feasibility
score, and expected validation budget, and rejects high-risk or low-confidence work fail-closed.

## Changes

### `oris-agent-contract` (0.5.0 — unchanged)

New public types in the autonomous-intake planning surface:

| Symbol | Kind | Description |
|--------|------|-------------|
| `AutonomousRiskTier` | `enum` | `Low / Medium / High` — orderable risk classification |
| `AutonomousPlanReasonCode` | `enum` | Stable discriminants: `Approved`, `DeniedHighRisk`, `DeniedLowFeasibility`, `DeniedUnsupportedClass`, `DeniedNoEvidence`, `UnknownFailClosed` |
| `AutonomousDenialCondition` | `struct` | Structured denial: `reason_code`, `description`, `recovery_hint` |
| `AutonomousTaskPlan` | `struct` | Full planning output: plan_id, dedupe_key, task_class, risk_tier, feasibility_score, validation_budget, expected_evidence, approved, reason_code, summary, denial_condition, fail_closed |
| `approve_autonomous_task_plan(…)` | `fn` | Constructor for approved plans |
| `deny_autonomous_task_plan(…)` | `fn` | Constructor for denied fail-closed plans |

### `oris-evokernel` (0.12.1 — unchanged)

- `EvoKernel::plan_autonomous_candidate(&DiscoveredCandidate) -> AutonomousTaskPlan` — new public method
- Internal: `autonomous_plan_for_candidate` + `autonomous_planning_params_for_class` private helpers
- Risk policy: `High` risk tier → denied; feasibility < 40 → denied; all denials are fail-closed

### Class–Risk mapping

| `BoundedTaskClass` | `AutonomousRiskTier` | Feasibility | Budget |
|--------------------|---------------------|-------------|--------|
| `LintFix` | Low | 85 | 2 |
| `DocsSingleFile` | Low | 90 | 1 |
| `DocsMultiFile` | Medium | 75 | 2 |
| `CargoDepUpgrade` | Medium | 70 | 3 |

## Tests

- 5 new regression tests in `oris-evokernel/tests/evolution_lifecycle_regression.rs` (prefix `autonomous_planning_`)
- 1 new wiring gate test `autonomous_task_planning_types_resolve` in `oris-runtime/tests/evolution_feature_wiring.rs`

## Validation

```
cargo fmt --all -- --check               ✓
cargo build --all --release --all-features  ✓
cargo test --release --all-features      ✓  (0 failures)
cargo publish -p oris-runtime --all-features --dry-run  ✓
```

## Closes

- Issue #265: `[EVO26-AUTO-02][P1] Bounded Task Planning and Risk Scoring For Autonomous Intake`


---

# Release v0.35.0

## oris-runtime v0.35.0

### Summary

Implements **AUTO-03: Autonomous Mutation Proposal Contracts** (Issue #266).

Adds the typed contract layer for autonomous mutation proposals, allowing the EvoKernel to produce structured `AutonomousMutationProposal` values with deterministic approval-mode routing (auto-approved vs. human-review) based on per-class risk tier.

### New Types (`oris-agent-contract`)

- `AutonomousApprovalMode` — `AutoApproved` | `RequiresHumanReview`
- `AutonomousProposalReasonCode` — `Proposed`, `DeniedPlanNotApproved`, `DeniedNoTargetScope`, `DeniedWeakEvidence`, `DeniedOutOfBounds`, `UnknownFailClosed`
- `AutonomousProposalScope` — `target_paths: Vec<String>`, `scope_rationale: String`, `max_files: u8`
- `AutonomousMutationProposal` — full proposal struct with `proposal_id`, `plan_id`, `dedupe_key`, `scope`, `expected_evidence`, `rollback_conditions`, `approval_mode`, `proposed`, `reason_code`, `summary`, `denial_condition`, `fail_closed`
- `approve_autonomous_mutation_proposal()` — constructor for an approved proposal
- `deny_autonomous_mutation_proposal()` — constructor for a denied proposal

### New Method (`oris-evokernel`)

- `EvoKernel::propose_autonomous_mutation(plan: &AutonomousTaskPlan) -> AutonomousMutationProposal`
  - Returns `AutoApproved` for `AutonomousRiskTier::Low` task classes
  - Returns `RequiresHumanReview` for `Medium`/`High` risk tier task classes
  - Denies with `DeniedPlanNotApproved` if the plan is not approved
  - Denies with `DeniedNoTargetScope` if the task class is unknown
  - All denials set `fail_closed = true`

### Risk-to-Approval Policy

| `BoundedTaskClass`  | `AutonomousRiskTier` | `AutonomousApprovalMode` |
|---------------------|----------------------|--------------------------|
| `LintFix`           | Low                  | `AutoApproved`           |
| `DocsSingleFile`    | Low                  | `AutoApproved`           |
| `DocsMultiFile`     | Medium               | `RequiresHumanReview`    |
| `CargoDepUpgrade`   | Medium               | `RequiresHumanReview`    |

### Tests

- 5 regression tests (`autonomous_proposal_*`) in `evolution_lifecycle_regression.rs`
- 1 wiring gate test `autonomous_mutation_proposal_types_resolve` in `evolution_feature_wiring.rs`

### Closes

- Issue #266 (AUTO-03)


---

# Release v0.36.0

## oris-runtime v0.36.0

### Summary

Implements **AUTO-04: Semantic Task-Class Generalization Beyond Normalized Signals** (Issue #267).

Adds a typed semantic equivalence layer that allows replay selection to generalize beyond exact normalized signal matching to broader task families. False-positive prevention is maintained: only explicitly approved families are replayed; unrelated tasks do not replay.

### New Types (`oris-agent-contract`)

- `TaskEquivalenceClass` — semantic family groupings: `DocumentationEdit`, `StaticAnalysisFix`, `DependencyManifestUpdate`, `Unclassified`
- `EquivalenceExplanation` — structured audit record: `task_equivalence_class`, `rationale`, `matching_features`, `replay_match_confidence`
- `SemanticReplayReasonCode` — `EquivalenceMatchApproved`, `LowConfidenceDenied`, `NoEquivalenceClassMatch`, `EquivalenceClassNotAllowed`, `UnknownFailClosed`
- `SemanticReplayDecision` — `evaluation_id`, `task_id`, `replay_decision`, `equivalence_explanation`, `reason_code`, `summary`, `fail_closed`
- `approve_semantic_replay()` — constructor for an approved decision
- `deny_semantic_replay()` — constructor for a denied decision (fail_closed=true)

### New Method (`oris-evokernel`)

- `EvoKernel::evaluate_semantic_replay(task_id, task_class: &BoundedTaskClass) -> SemanticReplayDecision`
  - Low-risk classes (`LintFix`, `DocsSingleFile`) → `EquivalenceMatchApproved`, `fail_closed=false`
  - Medium-risk classes (`DocsMultiFile`, `CargoDepUpgrade`) → `EquivalenceClassNotAllowed`, `fail_closed=true`
  - Returns full `EquivalenceExplanation` with matching_features and replay_match_confidence for approved decisions

### Semantic Equivalence Policy

| `BoundedTaskClass`  | `TaskEquivalenceClass`         | Auto-Replay | Confidence |
|---------------------|-------------------------------|-------------|------------|
| `LintFix`           | `StaticAnalysisFix`           | Yes         | 95         |
| `DocsSingleFile`    | `DocumentationEdit`           | Yes         | 90         |
| `DocsMultiFile`     | `DocumentationEdit`           | No (human review) | 75  |
| `CargoDepUpgrade`   | `DependencyManifestUpdate`    | No (human review) | 72  |

### Tests

- 5 regression tests (`semantic_replay_*`) in `evolution_lifecycle_regression.rs`
- 1 wiring gate test `semantic_replay_decision_types_resolve` in `evolution_feature_wiring.rs`

### Closes

- Issue #267 (AUTO-04)


---

# Release: oris-runtime v0.37.0

## Summary

Implements **AUTO-05 — Continuous Confidence Revalidation** for the autonomous
evolution pipeline.

Assets (genes and capsules) can now be continuously re-evaluated for replay
eligibility as runtime confidence signals accumulate. Failed revalidation
suspends replay automatically; assets with too many failures are demoted or
quarantined.

## New Types (`oris-agent-contract v0.5.3`)

| Type | Kind | Description |
|------|------|-------------|
| `ConfidenceState` | `enum` | Asset lifecycle states: `Active`, `Decaying`, `Revalidating`, `Demoted`, `Quarantined` |
| `RevalidationOutcome` | `enum` | Round outcome: `Passed`, `Failed`, `Pending`, `ErrorFailClosed` |
| `ConfidenceDemotionReasonCode` | `enum` | Reason codes for demotion: `ConfidenceDecayThreshold`, `RepeatedReplayFailure`, `MaxFailureCountExceeded`, `ExplicitRevocation`, `UnknownFailClosed` |
| `ReplayEligibility` | `enum` | `Eligible` / `Ineligible` after evaluation |
| `ConfidenceRevalidationResult` | `struct` | Full result of one revalidation run |
| `DemotionDecision` | `struct` | Asset demotion / quarantine transition record |

## New Constructors (`oris-agent-contract v0.5.3`)

| Constructor | Description |
|-------------|-------------|
| `pass_confidence_revalidation(…)` | Creates a passing result, restores `Active` state |
| `fail_confidence_revalidation(…)` | Creates a failing result, marks asset `Ineligible`, `fail_closed=true` |
| `demote_asset(…)` | Creates demotion decision; escalates to `Quarantined` when appropriate |

## New Kernel Methods (`oris-evokernel v0.12.4`)

| Method | Description |
|--------|-------------|
| `EvoKernel::evaluate_confidence_revalidation(asset_id, state, failures)` | Run a revalidation cycle; fail-closed at 3+ failures |
| `EvoKernel::evaluate_asset_demotion(asset_id, prior_state, failures, reason)` | Produce demotion decision; quarantine at 5+ failures |

## Policy

- **Pass threshold**: `failure_count < 3` → `Passed`, `Eligible`, `fail_closed=false`
- **Fail threshold**: `failure_count >= 3` → `Failed`, `Ineligible`, `fail_closed=true`
- **Demote vs Quarantine**: `failure_count < 5` → `Demoted`; `failure_count >= 5` → `Quarantined`

## Validation

- `cargo fmt --all -- --check` ✓
- 5 regression tests (`confidence_revalidation_*`) — all pass
- 1 wiring gate test (`confidence_revalidation_decision_types_resolve`) — pass
- `cargo build --all --release --all-features` ✓
- `cargo test --release --all-features` ✓
- `cargo publish -p oris-runtime --all-features --dry-run` ✓

## Changed Crates

| Crate | Old | New |
|-------|-----|-----|
| `oris-agent-contract` | 0.5.2 | 0.5.3 |
| `oris-evokernel` | 0.12.3 | 0.12.4 |
| `oris-runtime` | 0.36.0 | 0.37.0 |

## Linked Issue

Closes #269


---

# Release: oris-runtime v0.38.0

## Summary

Implements **AUTO-06 — Bounded Autonomous PR Lane For Low-Risk Task Classes**.

A narrowly scoped autonomous PR lane is now available for explicitly approved low-risk task classes (`DocsSingleFile`, `LintFix`). The lane gates on validated evidence; all other classes and missing evidence fail closed before any PR payload is assembled.

## New Types (`oris-agent-contract v0.5.4`)

| Type | Kind | Description |
|------|------|-------------|
| `AutonomousPrLaneStatus` | `enum` | `PrReady` / `Denied` |
| `PrLaneApprovalState` | `enum` | `ClassApproved` / `ClassNotApproved` |
| `AutonomousPrLaneReasonCode` | `enum` | `ApprovedForAutonomousPr`, `TaskClassNotApproved`, `PatchEvidenceMissing`, `ValidationEvidenceMissing`, `RiskTierTooHigh`, `UnknownFailClosed` |
| `PrEvidenceBundle` | `struct` | Evidence gate: patch summary, validation status, audit trail |
| `AutonomousPrLaneDecision` | `struct` | Full PR lane decision record with `branch_name`, `pr_payload`, `evidence_bundle` |

## New Constructors (`oris-agent-contract v0.5.4`)

| Constructor | Description |
|-------------|-------------|
| `approve_autonomous_pr_lane(…)` | Creates an approved decision with branch and PR payload |
| `deny_autonomous_pr_lane(…)` | Creates a denied decision, always `fail_closed=true` |

## New Kernel Method (`oris-evokernel v0.12.5`)

| Method | Description |
|--------|-------------|
| `EvoKernel::evaluate_autonomous_pr_lane(task_id, task_class, risk_tier, evidence_bundle)` | Gate for the bounded autonomous PR lane |

## Policy

- **Approved classes**: `DocsSingleFile`, `LintFix` at `AutonomousRiskTier::Low` with `validation_passed=true`
- **All other configurations**: `Denied`, `fail_closed=true`

## Validation

- `cargo fmt --all -- --check` ✓
- 5 regression tests (`autonomous_pr_lane_*`) — all pass
- 1 wiring gate test (`autonomous_pr_lane_decision_types_resolve`) — pass
- `cargo build --all --release --all-features` ✓
- `cargo test --release --all-features` ✓
- `cargo publish -p oris-runtime --all-features --dry-run` ✓

## Changed Crates

| Crate | Old | New |
|-------|-----|-----|
| `oris-agent-contract` | 0.5.3 | 0.5.4 |
| `oris-evokernel` | 0.12.4 | 0.12.5 |
| `oris-runtime` | 0.37.0 | 0.38.0 |

## Linked Issue

Closes #270


---

# Release v0.39.0

## Summary

EVO26-AUTO-07: Fail-Closed Autonomous Merge and Release Gate For Narrow Safe Lanes.

## Changes

### `oris-agent-contract` v0.5.5

- Added `AutonomousMergeGateStatus` enum: `MergeApproved`, `MergeBlocked`
- Added `AutonomousReleaseGateStatus` enum: `ReleaseApproved`, `ReleaseBlocked`
- Added `AutonomousPublishGateStatus` enum: `PublishApproved`, `PublishBlocked`
- Added `KillSwitchState` enum: `Inactive`, `Active`
- Added `AutonomousReleaseReasonCode` enum with 7 reason codes
- Added `RollbackPlan` struct
- Added `AutonomousReleaseGateDecision` struct (machine-readable gate decision record)
- Added `approve_autonomous_release_gate()` constructor
- Added `deny_autonomous_release_gate()` constructor

### `oris-evokernel` v0.12.6

- Added `EvoKernel::evaluate_autonomous_release_gate()` public method
- Added `autonomous_release_gate_decision()` private helper (fail-closed policy: only `DocsSingleFile` and `LintFix` at `Low` risk tier with inactive kill switch and complete evidence are approved)

### `oris-orchestrator` v0.3.2

- Updated `oris-agent-contract` dependency to v0.5.5
- Added `tests/autonomous_release_gate.rs` with 5 regression tests

### `oris-runtime` v0.39.0

- Updated `oris-evokernel` dependency to v0.12.6
- Added `autonomous_release_gate_decision_types_resolve` wiring gate test

## Validation

- `cargo fmt --all -- --check`
- `cargo test -p oris-orchestrator autonomous_release_ -- --nocapture` — 5 tests passing
- `cargo test -p oris-runtime --test evolution_feature_wiring --features full-evolution-experimental autonomous_release_gate_decision_types_resolve` — passing
- `cargo test --release --all-features` — 0 failures
- `cargo publish -p oris-runtime --all-features --dry-run` — passed
