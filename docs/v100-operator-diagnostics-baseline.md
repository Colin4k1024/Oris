# v1.0 Operator-Facing Diagnostics Baseline — Verification Report

**Issue:** #417 Operator-facing diagnostics adequate
**Parent Milestone:** v1.0 Trusted Improvement System (#369)
**Status:** Complete
**Date:** 2026-03-27

## Verification Summary

The operator-facing diagnostics for v1.0 are verified as adequate across all four areas specified in the issue:
- Structured tracing is in place
- Metrics and operational visibility are available
- Diagnostic quality for failure paths is strong
- Auditable evidence and policy outcomes exist

## Diagnostics Coverage Analysis

### 1. Structured Tracing

**Status:** In Place

**Evidence:**
- **Tracing crate:** The codebase uses the `tracing` crate consistently across examples and runtime
- **Span-based diagnostics:** `crates/oris-runtime/src/execution_server/api_handlers.rs:3725` uses `info_span!` macro for execution lifecycle tracing with:
  - `operation` - the operation being performed
  - `request_id` - unique request identifier
  - `trace_id` / `span_id` / `parent_span_id` - W3C trace context
  - `thread_id` / `attempt_id` / `worker_id` - execution context
- **Tracer integration:** Tracing spans are integrated with `TraceContextState` for distributed trace propagation

**Files with tracing:**
- `crates/oris-evokernel/src/adapters.rs` - Gene store and mutation evaluator adapters
- `crates/oris-runtime/src/execution_server/api_handlers.rs` - HTTP API execution lifecycle
- `crates/oris-runtime/src/vectorstore/surrealdb/surrealdb.rs` - SurrealDB vector store
- `crates/oris-mutation-evaluator/src/evaluator.rs` - Mutation evaluation
- Examples: `oris_starter_axum`, `execution_server`, `oris_worker_tokio`

### 2. Metrics and Operational Visibility

**Status:** Available

**Evidence:**
- **Scheduler Metrics:** `crates/oris-execution-runtime/src/scheduler.rs:407` - `SchedulerMetrics` struct provides:
  - `tenant_run_counts` - per-tenant run counts
  - `worker_lease_counts` - per-worker lease counts
  - `throttle_limits` - current throttle configuration
- **Metrics Endpoint:** `crates/oris-execution-runtime/src/api_contract.rs:127` - `/metrics` endpoint for Prometheus scraping
- **Confidence Metrics:** `crates/oris-evolution/src/confidence.rs:132` - `ConfidenceMetrics` struct tracks:
  - `decay_checks_total` - decay check counter
  - `capsules_decayed_total` - capsules affected by decay
  - `capsules_quarantined_total` - quarantined capsules
  - `confidence_boosts_total` - successful reuse boosts

### 3. Failure Path Diagnostics

**Status:** Strong

**Evidence:**
- **Evidence Bundle:** `crates/oris-intake/src/evidence.rs` provides comprehensive failure documentation:
  - `ValidationOutput` - command, exit_code, passed, stdout, stderr, duration_ms
  - `BeforeAfterResults` - files_changed, lines_added, lines_removed, summary
  - `EnvironmentContext` - rustc_version, cargo_lock_hash, target_triple, os, git_sha
- **Error handling:** Structured error types using `thiserror` across crates
- **API Error propagation:** `crates/oris-runtime/src/execution_server/api_handlers.rs` - detailed error context with `ApiError` enum

### 4. Auditable Evidence and Policy Outcomes

**Status:** Complete

**Evidence:**
- **EvidenceBundle:** `crates/oris-orchestrator/src/evidence.rs` - evidence bundling with `ValidationGate::is_pr_ready()` check
- **Evidence fields:**
  - `EvidenceBundle::build_ok` - build validation passed
  - `EvidenceBundle::contract_ok` - contract validation passed
  - `EvidenceBundle::e2e_ok` - end-to-end tests passed
  - `EvidenceBundle::backend_parity_ok` - backend parity validated
  - `EvidenceBundle::policy_ok` - policy gate passed
- **PolicyDecisionLink:** Records policy_name, decision, reasoning, timestamp
- **ConfidenceUpdate:** Tracks promotion/demotion with previous/new confidence and reason
- **EvidenceCompleteness:** Enum with Complete, Incomplete { missing_items }, Validated states

## Key Components

| Component | File | Purpose |
|-----------|------|---------|
| ExecutionLifecycle Span | `api_handlers.rs:3725` | Structured span for execution tracing |
| SchedulerMetrics | `scheduler.rs:407` | Operational metrics for scheduler |
| EvidenceBundle | `evidence.rs:18` | Complete evidence for proposals |
| EvidenceBundleBuilder | `evidence.rs:129` | Builder for assembling evidence |
| ValidationGate | `evidence.rs:37` | PR readiness validation |
| ConfidenceMetrics | `confidence.rs:132` | Confidence lifecycle metrics |
| PolicyDecisionLink | `evidence.rs:88` | Policy decision audit trail |

## Validation Results

- `cargo fmt --all -- --check` - Passed
- `cargo test -p oris-orchestrator --release` - 8 tests passed
- `cargo build --all --release --all-features` - Built successfully
- `cargo test --release --all-features` - All tests passed

## Architecture

```
Operator Dashboard
       |
       v
+------------------+
| Metrics Endpoint | <-- SchedulerMetrics, ConfidenceMetrics
+------------------+
       |
       v
+---------------+
| Tracing Spans | <-- ExecutionLifecycle, info_span!
+---------------+
       |
       v
+----------------------+
| Evidence Bundles     | <-- ValidationOutput, PolicyDecisionLink
+----------------------+
       |
       v
+----------------+
| Policy Outcomes | <-- Governor decisions, confidence updates
+----------------+
```

## Conclusion

The v1.0 operator-facing diagnostics baseline is verified as complete:
- Structured tracing with span-based execution lifecycle tracking
- Metrics endpoint with scheduler and confidence metrics
- Rich failure path diagnostics through evidence bundles
- Complete auditable evidence with policy decision links

The existing infrastructure supports operational visibility for the Trusted Improvement System at v1.0.

## Parent Milestone Exit Checklist

**Parent Milestone Exit Checklist:**
- [x] Operator-facing diagnostics adequate (this issue)
