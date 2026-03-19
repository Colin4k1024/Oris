# Oris Runtime — Release History

> Consolidated release notes. For the current release, see [RELEASE_v0.61.0.md](RELEASE_v0.61.0.md).

---

## v0.61.0 — A2A Economic Lifecycle Endpoints

**Summary**: Implements A2A Economic Lifecycle endpoints (EVOMAP-154, issue #334) covering service registration, discovery, bid submission, deterministic bid evaluation, and dispute rule querying, with 16 new integration tests.

### New Endpoints

| Method | Path | Description |
|--------|------|-------------|
| `POST` | `/a2a/service/register` | Register a node service offering (idempotent) |
| `GET`  | `/a2a/service/list` | List services with category/status/owner/query filters |
| `GET`  | `/a2a/service/:id` | Retrieve a specific service by ID |
| `POST` | `/a2a/bid/submit` | Submit a bid for a service contract (idempotent) |
| `GET`  | `/a2a/bid/:id` | Retrieve a specific bid by ID |
| `POST` | `/a2a/bid/evaluate` | Deterministically evaluate competing bids (strategy: highest_bid\|lowest_bid) |
| `GET`  | `/a2a/dispute/rule` | Query canonical ruleset or retrieve a specific stored dispute rule |

### Bid Evaluation

Deterministic settlement logic with two versioned strategies:
- `highest_bid` (default) — selects the open bid with the highest `amount`; ties broken by earliest submission
- `lowest_bid` — selects the open bid with the lowest `amount`; same tie-break

Result carries `schema_version: "v1"` for auditability.

### Dispute Rule Querying

`GET /a2a/dispute/rule` now:
- Without `?dispute_id` → returns the canonical ruleset definition (4 decisions + aliases)
- With `?dispute_id=<id>` → retrieves the specific stored rule record

### Tests Added (16)

**Service**: a2a_service_register_returns_service, a2a_service_register_missing_title_rejected, a2a_service_get_returns_service, a2a_service_get_unknown_returns_404, a2a_service_list_returns_services, a2a_service_list_category_filter

**Bid**: a2a_bid_submit_returns_bid, a2a_bid_get_returns_bid, a2a_bid_get_unknown_returns_404, a2a_bid_evaluate_returns_winner, a2a_bid_evaluate_lowest_bid_strategy, a2a_bid_evaluate_non_owner_forbidden, a2a_bid_evaluate_invalid_strategy_rejected, a2a_bid_evaluate_no_open_bids_returns_no_open_bids

**Dispute Rule**: a2a_dispute_rule_get_returns_ruleset, a2a_dispute_rule_get_by_id_returns_stored_rule

### Validation

- `cargo fmt --all -- --check` ✅
- `cargo test -p oris-runtime --features "sqlite-persistence,execution-server,a2a-production" a2a_service_` → 6/6 ✅
- `cargo test -p oris-runtime --features "sqlite-persistence,execution-server,a2a-production" a2a_bid_` → 8/8 ✅
- `cargo test -p oris-runtime --features "sqlite-persistence,execution-server,a2a-production" a2a_dispute_rule` → 2/2 ✅
- `cargo build --all --release --all-features` ✅
- `cargo test --release --all-features` → 0 failures ✅
- `cargo publish --dry-run` ✅

---

## v0.60.0 — A2A Project Workflow Endpoints

**Summary**: Implements the A2A Project Workflow endpoints (EVOMAP-153, issue #333) with 11 new integration tests covering create, detail, state transitions, per-project suggestions, and list operations.

### New Endpoints

| Method | Path | Description |
|--------|------|-------------|
| `POST` | `/a2a/project/create` | Create a project (idempotent by proposer+title+summary+tags) |
| `GET`  | `/a2a/project/:id` | Retrieve a project by ID |
| `POST` | `/a2a/project/:id/state` | Transition lifecycle state (active/paused/completed) |
| `GET`  | `/a2a/project/:id/suggestions` | Get suggestions scoped to a specific project |
| `GET`  | `/a2a/project/list` | List projects with optional status/owner_id/query/limit/offset filters |

### Data Model

- `EvomapProjectRecord` gains a new `lifecycle_state: Option<String>` field
- `evomap_project_json()` helper now serializes `lifecycle_state`
- `evomap_project_propose` initializes records with `lifecycle_state: Some("active")`

### Tests Added (11)

a2a_project_create_returns_project, a2a_project_create_missing_title_rejected, a2a_project_get_returns_project, a2a_project_get_unknown_returns_404, a2a_project_state_active, a2a_project_state_paused, a2a_project_state_completed, a2a_project_state_invalid_rejected, a2a_project_list_get_returns_list, a2a_project_list_get_status_filter, a2a_project_id_suggestions_returns_suggestions

### Validation

- `cargo fmt --all -- --check` ✅
- `cargo test -p oris-runtime --features "sqlite-persistence,execution-server,a2a-production" a2a_project_` → 11/11 ✅
- `cargo build --all --release --all-features` ✅
- `cargo test --release --all-features` → 0 failures ✅

---

## v0.59.0 — A2A Council Workflow Contract Tests

**Summary**: Adds A2A Council Workflow contract tests (EVOMAP-152, issue #332). Closes #332.

### New Tests — A2A Council Workflow (13)

**Session management**: a2a_council_session_open_returns_ok, a2a_council_session_open_idempotent_same_settings, a2a_council_session_invalid_action_rejected, a2a_council_session_close_returns_ok

**Proposal submission**: a2a_council_propose_missing_title_rejected, a2a_council_propose_records_proposal, a2a_council_propose_idempotent_on_repeat

**Voting**: a2a_council_vote_records_vote, a2a_council_vote_idempotent_on_repeat, a2a_council_vote_conflict_rejected

**Execution**: a2a_council_execute_insufficient_quorum_rejected, a2a_council_execute_approved_proposal_succeeds, a2a_council_execute_idempotent_on_repeat

### Validation

- `cargo fmt --all -- --check` ✅
- `cargo test -p oris-runtime --features "sqlite-persistence,execution-server,a2a-production" a2a_council_` → 13/13 ✅
- `cargo build --all --release --all-features` ✅
- `cargo test --release --all-features` → 0 failures ✅

---

## v0.58.0 — A2A Asset Detail and Governance

**Summary**: Adds 13 contract tests covering A2A asset detail and governance semantics for issue #331.

### Tests Added

a2a_asset_detail_requires_sender_id, a2a_asset_detail_unknown_asset_returns_404, a2a_asset_detail_returns_full_shape, a2a_asset_detail_verify_records_verification, a2a_asset_detail_verify_idempotent_on_repeat, a2a_asset_detail_verify_rejects_invalid_status, a2a_asset_detail_vote_records_vote, a2a_asset_detail_vote_idempotent_on_repeat, a2a_asset_detail_vote_rejects_invalid_vote_value, a2a_asset_detail_audit_trail_reflects_governance_events, a2a_asset_detail_reviews_returns_reviews_shape, a2a_asset_detail_governance_worker_role_rejected_on_verify, a2a_asset_detail_governance_worker_role_rejected_on_vote

### Validation

- `cargo fmt --all -- --check` ✓
- `cargo test -p oris-runtime --features "sqlite-persistence,execution-server,a2a-production" a2a_asset_detail_` → 13 passed ✓
- `cargo build --all --release --all-features` ✓
- `cargo test --release --all-features` ✓

---

## v0.57.0 — A2A Asset Discovery API

**Summary**: Adds 13 contract tests covering the A2A Asset Discovery API endpoints (issue #330).

### Tests Added

a2a_assets_search_requires_sender_id, a2a_assets_search_empty_sender_id_rejected, a2a_assets_search_requires_handshake, a2a_assets_search_returns_results_shape, a2a_assets_search_mode_field_is_search, a2a_assets_ranked_returns_ranked_mode, a2a_assets_explore_returns_explore_mode, a2a_assets_recommended_returns_recommended_mode, a2a_assets_trending_returns_trending_mode, a2a_assets_categories_returns_categories_shape, a2a_assets_search_pagination_limit_respected, a2a_assets_ranked_deterministic_for_same_inputs, a2a_assets_search_idempotent_flag_is_true

### Validation

- `cargo fmt --all -- --check` ✓
- `cargo test -p oris-runtime --features "sqlite-persistence,execution-server,a2a-production" a2a_assets_` → 13 passed ✓
- `cargo build --all --release --all-features` ✓
- `cargo test --release --all-features` ✓

---

## v0.56.0 — A2A Task Lifecycle Semantics

**Summary**: Implements the A2A task lifecycle semantics (EVOMAP-149 / issue #329) and fixes a pre-existing test failure caused by the lifecycle route missing under the `a2a-production` feature set.

### Bug Fix

The `/v1/evolution/a2a/tasks/:task_id/lifecycle` route was previously registered only in `with_evolution_routes`. Fixed to also register in `with_a2a_routes` under `a2a-production`.

### Tests — A2A task lifecycle (18 new)

| Endpoint | Tests added |
|---|---|
| `POST /a2a/task/submit` | 4 |
| `GET /a2a/task/list` | 3 |
| `GET /a2a/task/:id` | 2 |
| `GET /a2a/task/my` | 2 |
| `GET /a2a/task/eligible-count` | 2 |
| `POST /a2a/task/release` | 2 |
| `POST /a2a/ask` | 2 |
| Full lifecycle | 1 |

### Validation

- `cargo fmt --all -- --check` — clean
- `cargo test -p oris-runtime --features "sqlite-persistence,execution-server,a2a-production" a2a_task_` — 21/21 pass
- `cargo build --all --release --all-features` — clean
- `cargo test --release --all-features` — 0 failures

---

## v0.55.0 — A2A Protocol Core Semantics

**Summary**: Adds 16 unit tests covering the A2A protocol core semantics exposed under the `a2a-production` feature flag (EVOMAP-148 / issue #328).

### Test Coverage

| Endpoint | Tests added |
|---|---|
| `POST /a2a/validate` | 5 |
| `POST /a2a/report` | 4 |
| `POST /a2a/decision` | 3 |
| `POST /a2a/revoke` | 2 |
| `GET /a2a/policy/model-tiers` | 2 |

### Validation

- `cargo fmt --all -- --check` — clean
- `cargo test -p oris-runtime --features "sqlite-persistence,execution-server,a2a-production"` — 16/16 pass
- `cargo build --all --release --all-features` — clean
- `cargo test --release --all-features` — 0 failures

---

## v0.54.0 — Fail-Closed Autonomous Merge and Release Gate

**Summary**: Adds the fail-closed Autonomous Merge and Release Gate for narrow approved task classes (issue #327, EVO26-AUTO-07).

### New Gates

- **`MergeGate`** — enforces kill switch, class eligibility, risk-tier ceiling, and complete evidence
- **`ExtendedReleaseGate`** — re-checks kill switch, verifies merge gate result, rejects post-gate state drift
- **`GatedPublishGate`** — re-checks kill switch, requires release gate approval, mandates validated `RollbackPlan`

### Approved Task Classes

Only three narrowest, lowest-risk: `missing-import`, `type-mismatch`, `test-failure`.

### Changed Crates

| Crate | Old | New |
|---|---|---|
| `oris-runtime` | 0.53.0 | 0.54.0 |
| `oris-orchestrator` | 0.4.3 | 0.5.0 |

### Validation

- `cargo fmt --all -- --check` ✓
- `cargo test -p oris-orchestrator autonomous_release_` → 44 new tests ✓
- `cargo build --all --release --all-features` ✓
- `cargo test --release --all-features` → 0 failures ✓

---

## v0.53.0 — Bounded Autonomous PR Lane

**Summary**: oris-runtime now exposes a fail-closed autonomous PR lane that prepares deterministic branch and evidence-backed PR artifacts only for explicitly approved low-risk task classes.

### What's in this release

- Added machine-readable autonomous PR lane contracts
- Added EvoKernel autonomous PR lane gate for low-risk docs and lint tasks
- Kept the autonomous PR lane wired through the runtime facade

### Validation

- `cargo test -p oris-orchestrator autonomous_pr_` — all pass
- `cargo test -p oris-runtime --test evolution_feature_wiring` — pass
- `cargo build --all --release --all-features` — pass
- `cargo test --release --all-features` — pass

---

## v0.52.0 — Continuous Confidence Revalidation

**Summary**: oris-runtime now exposes continuous confidence revalidation and deterministic asset demotion decisions so stale or repeatedly failing reusable assets automatically lose replay eligibility.

### What's in this release

- Added confidence lifecycle contracts covering revalidation result, replay eligibility, demotion decision, quarantine transition
- Added EvoKernel entrypoints for confidence revalidation and asset demotion

### Validation

- `cargo test -p oris-evokernel --test evolution_lifecycle_regression confidence_revalidation_` — pass
- `cargo build --all --release --all-features` — pass
- `cargo test --release --all-features` — pass

---

## v0.51.0 — Semantic Replay Task-Class Generalization

**Summary**: oris-runtime now exposes deterministic semantic replay decisions for bounded task families so replay can generalize beyond exact normalized signals.

### What's in this release

- Added semantic replay decision contracts for task equivalence class, equivalence explanation, replay confidence
- Added EvoKernel semantic replay evaluation for approved low-risk task families

### Validation

- `cargo test -p oris-evokernel --test evolution_lifecycle_regression semantic_replay_` — pass
- `cargo build --all --release --all-features` — pass
- `cargo test --release --all-features` — pass

---

## v0.50.0 — Autonomous Mutation Proposal Contracts

**Summary**: oris-runtime now turns approved autonomous task plans into bounded, machine-readable mutation proposals.

### What's in this release

- Added autonomous mutation proposal generation through the EvoKernel autonomous proposal entrypoint
- Added bounded proposal scope, expected evidence, rollback conditions, approval mode

### Validation

- `cargo fmt --all -- --check`
- `cargo test -p oris-evokernel --test evolution_lifecycle_regression autonomous_proposal_` — pass
- `cargo build --all --release --all-features` — pass
- `cargo test --release --all-features` — pass

---

## v0.49.0 — Evolution Network Security Hardening

**Summary**: Hardens evolution-network capsule ingestion with signed envelopes, per-peer rate limiting, and append-only network audit logs.

### What's in this release

- Added Ed25519 envelope signing helpers and signature verification
- Added per-peer capsule rate limiting and structured ACCEPT/REJECT audit logging
- Added optional `network-mtls` feature flag

### Validation

- `cargo fmt --all -- --check`
- `cargo test -p oris-evolution-network`
- `cargo build --all --release --all-features`
- `cargo test --release --all-features`

---

## v0.48.0 — Gossip Sync Engine

**Summary**: Adds an operational push-pull gossip sync engine for the evolution network (issue #303).

### Changes

- Added `gossip::GossipSyncEngine` with digest-based push-pull sync
- Added `GossipConfig`, `GossipDigest`, `GossipDigestEntry`, `GossipSyncReport`
- Added threshold-based digest filtering
- Added optional `gossip-msgpack` feature via `rmp-serde`

### Validation

- `cargo test -p oris-evolution-network`
- `cargo build --all --release --all-features`

---

## v0.47.0 — Evolution Scenario Example Suite

**Summary**: Adds runnable end-to-end demos under `examples/evo_oris_repo/` (issue #302).

### Changes

- Added `intake_webhook_demo` for GitHub Actions failure webhook simulation
- Added `confidence_lifecycle_demo` for confidence curve visualization
- Completed `network_exchange` for remote capsule transfer demonstration

### Validation

- `cargo fmt --all -- --check`
- `cargo build -p evo_oris_repo --all-features`
- `cargo run -p evo_oris_repo --bin intake_webhook_demo --all-features`
- `cargo run -p evo_oris_repo --bin confidence_lifecycle_demo --all-features`
- `cargo run -p evo_oris_repo --bin network_exchange --all-features`

---

## v0.46.0 — Remote Capsule Auto-Promotion Pipeline

**Summary**: Adds automatic quarantine decision path for remote capsules (issue #301).

### Changes

- Added `RemoteCapsuleReceiver::on_capsule_received()`
- Added configurable `PROMOTE_THRESHOLD` support (default 0.70)
- Added `CapsuleDisposition` (Promoted / Quarantined)
- Added append-only JSONL audit logging

### Validation

- `cargo fmt --all -- --check`
- `cargo test -p oris-evolution-network`
- `cargo test --release --all-features`

---

## v0.45.0 — OS-level Sandbox Isolation

**Summary**: Adds OS-level resource isolation to the sandbox executor (issue #300).

### Changes

- **`SandboxPolicy.max_memory_bytes`** — `RLIMIT_AS` on Linux
- **`SandboxPolicy.max_cpu_secs`** — `RLIMIT_CPU` on Linux
- **`SandboxPolicy.use_process_group`** — kills entire process group on timeout
- **`resource-limits` feature flag** — opt-in via `features = ["resource-limits"]`

### Crate Versions

| Crate | Version |
|---|---|
| `oris-sandbox` | 0.3.0 |
| `oris-evolution-network` | 0.4.1 |
| `oris-runtime` | 0.44.0 |

### Validation

- `cargo fmt --all -- --check` ✅
- `cargo test -p oris-sandbox --features resource-limits --lib` → 6 passed ✅
- `cargo build --all --release --all-features` ✅

---

## v0.44.0 — Confidence Model: Time-Decay and Bayesian Update

**Summary**: Implements time-decay weighting and Bayesian conjugate update for mutation confidence scoring (issue #299).

### Changes

- **`CompositeScore`** — bundles raw score, time-decay-weighted score, Wilson score confidence interval
- **`BayesianConfidenceUpdater`** — Beta-Bernoulli conjugate model for tracking asset confidence
- **`builtin_priors()`** — returns canonical weak-success prior Beta(2, 1)

### Validation

- `cargo fmt --all -- --check` ✅
- `cargo test -p oris-mutation-evaluator --lib` → 18 passed ✅
- `cargo test -p oris-evolution --lib` → 91 passed ✅
- `cargo build --all --release --all-features` ✅

---

## v0.43.0 — Automatic Task Class Inference

**Summary**: Implements automatic task class inference via keyword recall scoring (issue #298).

### Changes

- **`TaskClassDefinition`** — extended with `description` field
- **`TaskClassInferencer`** — infers task class using keyword recall scoring
- **`load_task_classes()`** — loads from `~/.oris/oris-task-classes.toml`

### Validation

- `cargo fmt --all -- --check`
- `cargo test -p oris-evolution --features evolution-experimental` — 86 tests pass
- `cargo build --all --release --all-features`
- `cargo test --release --all-features`

---

## v0.42.0 — Intake-Driven Detect Stage Integration

**Summary**: Runtime diagnostics and webhook-derived failures now feed the evolution loop through the standard detect stage.

### What's in this release

- Added `detect_from_intake_events` and `intake_events_to_extractor_input` in oris-evokernel
- Added compiler-diagnostic and runtime-panic coverage for detect-to-select path

### Validation

- `cargo fmt --all -- --check`
- `cargo test -p oris-evokernel --test evolution_feature_wiring` — pass
- `cargo build --all --release --all-features` — pass
- `cargo test --release --all-features` — pass

---

## v0.41.0 — Circuit Breaker

**Summary**: Adds a three-state circuit breaker to the worker execution chain (issue #294).

### New: `CircuitBreaker`

- `CircuitState` enum — `Closed`, `Open { opened_at }`, `HalfOpen`
- `CircuitBreaker::trip()` — forces the breaker to `Open`
- `is_open()` — returns `true` while open; auto-transitions to `HalfOpen` after probe window

### Updated: `WorkerHealthTracker` and `SkeletonScheduler`

- `with_circuit_breaker()` builder methods
- `dispatch_one` returns `SchedulerDecision::Backpressure` when breaker is `Open`

### Validation

- `cargo fmt --all -- --check` ✓
- `cargo test -p oris-execution-runtime` — 27/27 passed ✓
- `cargo build --all --release --all-features` ✓

---

## v0.39.0 — Consolidated Release History (v0.1.0 through v0.39.0)

For detailed historical releases from v0.1.0 through v0.39.0, see [RELEASE_v0.39.0.md](RELEASE_v0.39.0.md).

---

## Common Validation Baseline

- `cargo fmt --all -- --check`
- `cargo build --all --release --all-features`
- `cargo test --release --all-features`

## Links

- **Crate:** [crates.io/crates/oris-runtime](https://crates.io/crates/oris-runtime)
- **Docs:** [docs.rs/oris-runtime](https://docs.rs/oris-runtime)
- **Repo:** [github.com/Colin4k1024/Oris](https://github.com/Colin4k1024/Oris)
