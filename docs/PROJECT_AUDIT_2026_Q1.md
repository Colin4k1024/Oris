# Oris Project Audit — 2026 Q1

**Date:** 2026-03-20
**Scope:** Full workspace audit of code quality, architecture health, and strategic gap identification
**Baseline version:** oris-runtime v0.61.0

---

## 1. Executive Summary

Oris is at a strong inflection point. The supervised self-evolution closed loop is implemented and validated (replay-driven mutation capture, bounded intake, auditable proposals, fail-closed execution, acceptance gating, quarantined remote reuse). The A2A protocol semantic surface is complete through v0.61.0 (protocol core, task lifecycle, asset discovery, council/project workflows, economic lifecycle). All 340+ historical issues are closed with zero currently open.

**Key findings from this audit:**

| Area | Status | Priority |
|------|--------|----------|
| Build health | 🔴 Compilation errors found and fixed (LeaseRecord fields, duplicate imports) | Fixed in this audit |
| Code quality | 🟢 Excellent (1 TODO, 0 FIXME, 1 unsafe block — justified) | Maintenance |
| Test coverage | 🟢 302+ unit tests; execution-server smoke tests added (A-6 ✅); CodeCov CI integrated (A-9 ✅) | Low |
| Error handling | 🟢 Confirmed: all unwrap() in evokernel/runtime are in #[cfg(test)] blocks; production paths clean | Resolved |
| Documentation | 🟢 17/17 crates have module docs; +511 lines public API doc comments (docs.rs) | Resolved |
| Architecture | 🟢 Clean DAG, no circular deps, good layering | Healthy |
| Deprecation debt | 🟢 0 deprecated items confirmed after audit (apparent 10 were feature-flag dead code, not live deprecations) | Resolved |
| Autonomy gaps | 🟡 7 layers missing for full autonomous operation | Strategic |

---

## 2. Codebase Metrics

| Metric | Value |
|--------|-------|
| Workspace crates | 18 (17 library + 1 server) |
| Example projects | 6 |
| Rust source files | 747 |
| Lines of code | ~185,000 |
| Functions (approx) | ~3,285 (42% async) |
| Unit tests | 295 |
| TODO comments | 1 |
| FIXME comments | 0 |
| Unsafe blocks | 1 (justified — sandbox syscall) |
| Deprecated items | 10 |
| Feature flags | 50+ |

### Crate Sizes

| Crate | Files | LOC | Tests | Test Ratio |
|-------|-------|-----|-------|------------|
| oris-runtime | 584 | 111,867 | 189 | 0.32 |
| oris-evokernel | 8 | 18,260 | 10 | 1.25 |
| oris-execution-runtime | 16 | 13,655 | 34 | 2.13 |
| oris-orchestrator | 31 | 8,538 | 20 | 0.65 |
| oris-evolution | 12 | 7,282 | 10 | 0.83 |
| oris-kernel | 31 | 6,037 | 21 | 0.68 |
| oris-intake | 9 | 3,992 | 10 | 1.11 |
| oris-evolution-network | 5 | 2,693 | 6 | 1.20 |
| oris-agent-contract | 1 | 2,394 | 1 | 1.00 |
| oris-mutation-evaluator | 7 | 1,583 | 8 | 1.14 |
| oris-genestore | 5 | 1,470 | 3 | 0.60 |
| oris-economics | 1 | 729 | 1 | 1.00 |
| oris-sandbox | 4 | 699 | 4 | 1.00 |
| oris-governor | 2 | 510 | 2 | 1.00 |
| oris-spec | 1 | 193 | 1 | 1.00 |
| oris-execution-server | 1 | 37 | 0 | 0.00 |

---

## 3. Build Health Issues (Fixed)

### 3.1 LeaseRecord Missing Fields (E0063)

**Crate:** `oris-execution-runtime`
**File:** `sqlite_runtime_repository.rs` (lines 538, 569, 2984)
**Root cause:** `LeaseRecord` struct was extended with `terminal_state` and `terminal_at` fields (K5-a lease finalization) but three construction sites were not updated.
**Fix:** Added `terminal_state: None, terminal_at: None` to all three `LeaseRecord` initializers.

### 3.2 Duplicate Imports (E0252)

**Crate:** `oris-execution-runtime`
**File:** `scheduler.rs` (lines 6-10)
**Root cause:** `HashMap`, `HashSet`, and `Arc` were imported twice, likely from a merge artifact.
**Fix:** Removed duplicate imports; removed unused `HashSet` from top-level scope (only used in test module).

### 3.3 Formatting

**File:** `lease.rs`
**Fix:** Applied `cargo fmt --all` to normalize whitespace.

---

## 4. Architecture Health

### 4.1 Dependency Graph (Clean DAG ✅)

```
Leaf crates (no workspace deps):
  oris-agent-contract, oris-economics, oris-genestore, oris-kernel, oris-mutation-evaluator

Layer 1:
  oris-evolution → oris-kernel
  oris-execution-runtime → oris-kernel
  oris-governor → oris-evolution
  oris-intake → oris-agent-contract, oris-evolution
  oris-sandbox → oris-evolution
  oris-spec → oris-evolution

Layer 2:
  oris-evolution-network → oris-evolution
  oris-orchestrator → oris-agent-contract, oris-evolution, oris-intake

Layer 3:
  oris-evokernel → 11 crates (highest fan-in)
  oris-runtime → oris-evokernel, oris-execution-runtime, oris-kernel

Layer 4:
  oris-execution-server → oris-runtime
```

**No circular dependencies.** Clean layer separation maintained.

### 4.2 Public API Surface

All 16 crates have well-structured `pub mod` / `pub use` re-exports. The only gap is `oris-orchestrator` which lacks `//!` module-level documentation in `lib.rs`.

---

## 5. Code Quality Findings

### 5.1 Error Handling — Unwrap Density

| Crate | unwrap() count | Severity |
|-------|---------------|----------|
| oris-evokernel | 155 (mostly in core.rs) | 🔴 High |
| oris-runtime | 134 (across multiple modules) | 🔴 High |
| oris-orchestrator | 22 | 🟡 Medium |
| oris-kernel | 18 | 🟡 Medium |
| oris-execution-runtime | 3 | 🟢 Low |

**Key risk files:**
- `oris-evokernel/src/core.rs` — 155 unwraps in metrics/snapshot/mutation paths
- `oris-runtime/src/interrupt.rs` — 10 unwraps
- `oris-runtime/src/postgres_store.rs` — 7 unwraps
- `oris-runtime/src/replay_verifier.rs` — 7 unwraps
- `oris-orchestrator/src/release_executor.rs` — 11 unwraps

### 5.2 Debug Output in Production Code

12 `println!`/`dbg!` calls in `crates/oris-runtime/src/llm/openai/mod.rs` should be converted to proper `tracing` log calls.

### 5.3 Deprecated Items Pending Migration

| Item | Location | Migration Target |
|------|----------|-----------------|
| Kernel module re-exports (4) | `oris-runtime/src/kernel/mod.rs` | `oris_runtime::execution_server::*` |
| Execution runtime re-exports (2) | `oris-runtime/src/execution_runtime.rs` | `oris_runtime::execution_server::*` |
| Tokenizer methods (2) | `text_splitter/*.rs` | `SplitterOptions::get_tokenizer_from_str` |
| QdrantClient re-export (1) | `vectorstore/qdrant/qdrant.rs` | `Qdrant` directly |
| `execute_stage()` (1) | `oris-evolution/src/pipeline.rs` | `execute()` |

### 5.4 Suppressed Warnings

24 `#[allow(...)]` annotations across 15 files — most are justified (`unused_imports` behind feature flags, `ambiguous_glob_reexports` for compat). No action needed.

---

## 6. Test Coverage Gaps

### 6.1 Crates Without Integration Tests

| Crate | Unit Tests | Integration Tests | Action |
|-------|-----------|------------------|--------|
| oris-execution-server | 7 | 7 | ✅ Smoke tests added (A-6) |
| oris-agent-contract | 1 | 0 | Should have contract tests |
| oris-economics | 1 | 0 | Should have model validation tests |
| oris-evolution | 10 | 0 | Should have pipeline integration tests |
| oris-evolution-network | 6 | 0 | Should have network protocol tests |
| oris-execution-runtime | 34 | 0 | Good unit coverage, integration tests optional |
| oris-genestore | 3 | 0 | Should have persistence round-trip tests |
| oris-spec | 1 | 0 | Low priority |

### 6.2 Missing Coverage Infrastructure

- No `tarpaulin` or `codecov` integration
- No coverage thresholds or gates in CI
- No per-crate coverage reports

---

## 7. Strategic Gap Analysis

### 7.1 Current Product Boundary

> **Accurate:** "Oris supports a supervised closed-loop self-evolution path with bounded acceptance gating."
>
> **Not yet accurate:** "Oris is a fully autonomous self-improving development and release system."

### 7.2 Completed Work Streams (All Closed)

| Stream | Issues | Status |
|--------|--------|--------|
| EVO-01 to EVO-05: Core evolution | #87-#91 | ✅ Shipped v0.14.0 |
| P1 to P3 Phase work | #296-#307 | ✅ Closed |
| EVO26-AUTO-01 to AUTO-13: Autonomy streams | #264-#285, #321-#327 | ✅ Closed |
| EVOMAP-148 to 155: A2A semantic parity | #328-#335 | ✅ Closed |
| KERNEL-K1 to K5: Kernel hardening | #336-#340 | ✅ Closed |

### 7.3 Seven Autonomy Layers (Gap-to-Close)

| Layer | Gap | Current State | Next Step |
|-------|-----|--------------|-----------|
| 1. Autonomous Intake | CI/alert signal discovery | Bounded caller-provided only | CI failure parser, test regression detector |
| 2. Task Planning | Classification, feasibility, blast-radius | None | Risk scoring model, budget constraints |
| 3. Proposal Generation | Self-generated work proposals | Mutation proposals only | End-to-end proposal-to-evidence pipeline |
| 4. Semantic Generalization | Broader task-class matching | Normalized signal equivalence | Embedding-based semantic matching |
| 5. Confidence Control | Decay, revalidation, demotion | Static confidence | Shadow revalidation daemon |
| 6. Autonomous Delivery | PR/release lane | Branch artifacts only | GitHub API integration, safe merge lanes |
| 7. Operational Governance | Policy, kill switches, rollback | Fail-closed basic | Risk-tiered policy engine |

### 7.4 Kernel 2.0 Maturity

Phase 1 kernel issues (K1-K5) addressed:
- K1: ExecutionStep contract freeze, effect capture, determinism guard
- K2: Canonical log store, replay cursor, replay verification, branch replay
- K3: Interrupt object, suspension state machine, replay-based resume
- K4: Plugin categories, determinism declarations, execution sandbox, version negotiation
- K5: Lease-based finalization, zero-data-loss recovery, context-aware scheduler, backpressure

**Remaining:** Production stress testing, multi-backend parity (Postgres vs SQLite), crash-recovery regression hardening.

---

## 8. Proposed Issue Directions

### Track A: Code Health and Reliability (P1)

| # | Title | Scope | Effort |
|---|-------|-------|--------|
| A-1 | Replace unwrap() with proper error handling in oris-evokernel/core.rs | 155 unwraps → Result-based error handling | M |
| A-2 | Replace unwrap() with proper error handling in oris-runtime hot paths | interrupt.rs, postgres_store.rs, replay_verifier.rs | M |
| A-3 | Replace println!/dbg! with tracing in oris-runtime LLM module | 12 debug calls → structured logging | S |
| A-4 | Add module-level documentation to oris-orchestrator | Missing `//!` docs in lib.rs | S |
| A-5 | Remove deprecated kernel/execution_runtime re-exports | 10 deprecated items with migration path | S |
| A-6 | ~~Add integration tests for oris-execution-server~~ | ✅ 7 smoke tests added (commit 9a98758) | M |
| A-7 | Add integration tests for oris-genestore | Persistence round-trip validation | S |
| A-8 | Add integration tests for oris-economics | Economic model validation | S |
| A-9 | ~~Integrate tarpaulin/codecov for coverage tracking~~ | ✅ CodeCov CI step + README badge added (commit e3cca72) | M |

### Track B: Kernel and Runtime Hardening (P1)

| # | Title | Scope | Effort |
|---|-------|-------|--------|
| B-1 | Postgres backend parity for EvoMap schema | Many TODO methods in Postgres runtime repo | L |
| B-2 | Crash-recovery stress testing for SQLite backend | Verify WAL mode correctness under concurrent load | M |
| B-3 | Lease finalization terminal state persistence | Wire terminal_state/terminal_at to SQLite schema | M |
| B-4 | Scheduler fairness validation under load | Stress test weighted priority dispatch | M |
| B-5 | Backpressure engine integration test | Per-tenant and per-worker throttle under saturation | M |

### Track C: Evolution Autonomy Expansion (P2)

| # | Title | Scope | Effort |
|---|-------|-------|--------|
| C-1 | CI failure intake parser (autonomous intake layer) | Parse test/compile/lint failures into structured candidates | L |
| C-2 | Confidence decay daemon (continuous confidence control) | Background revalidation with automatic demotion | L |
| C-3 | Embedding-based task-class matching (semantic generalization) | Beyond normalized signal equivalence | L |
| C-4 | Agent feedback loop closure (replay → reasoning reduction) | Replay results reduce future LLM reasoning | M |
| C-5 | Bounded PR lane (autonomous delivery) | Safe GitHub API merge for low-risk changes | L |

### Track D: Developer Experience (P2)

| # | Title | Scope | Effort |
|---|-------|-------|--------|
| D-1 | Per-crate examples for genestore, governor, intake | Missing examples directory | M |
| D-2 | Quickstart tutorial for self-evolution pipeline | End-to-end walkthrough | M |
| D-3 | API reference documentation generation | rustdoc enhancement | M |
| D-4 | Operator CLI improvements | Richer status/debugging commands | M |

### Track E: Production Readiness (P3)

| # | Title | Scope | Effort |
|---|-------|-------|--------|
| E-1 | Observability: OpenTelemetry trace integration | Structured tracing for production debugging | L |
| E-2 | Prometheus metrics endpoint | Runtime performance metrics exposure | M |
| E-3 | Rate limiting and abuse prevention | Per-tenant API rate limits | M |
| E-4 | Schema migration framework hardening | Version compatibility and rollback | M |

---

## 9. Recommended Execution Order

**Immediate (this sprint):**
1. A-3: Replace debug output with tracing (quick win)
2. A-4: Add orchestrator module docs (quick win)
3. A-5: Remove deprecated re-exports (quick win)
4. B-3: Wire lease terminal state to persistence

**Next sprint:**
5. A-1: Evokernel unwrap cleanup
6. A-2: Runtime unwrap cleanup
7. A-6: Execution server tests
8. B-1: Postgres backend parity

**Medium term (1-2 months):**
9. A-9: Coverage infrastructure
10. C-1: CI failure intake parser
11. C-2: Confidence decay daemon
12. C-4: Agent feedback loop

**Strategic (3-6 months):**
13. C-3: Embedding-based task-class matching
14. C-5: Bounded PR lane
15. E-1: OpenTelemetry integration
16. E-2: Prometheus metrics

---

## 10. Strategic Direction Summary

The project's evolution should follow this ordered priority:

1. **First: Make the runtime production-reliable** — Fix compilation issues, clean up error handling, remove deprecated code, ensure all backends work.
2. **Second: Strengthen the test safety net** — Close integration test gaps, add coverage tracking, ensure regression gates catch regressions.
3. **Third: Expand the autonomy boundary incrementally** — Each layer (intake → planning → proposals → delivery) builds on the previous. Weak replay trust compounds error in later stages.
4. **Fourth: Developer experience** — Examples, tutorials, and documentation make the project accessible for adoption.
5. **Fifth: Production observability and operations** — Tracing, metrics, and operational tooling for real-world deployment.

> **North-Star Outcome:**
> Task → Detect → Replay if trusted → Mutate only when needed → Validate → Capture → Reuse → Reduce reasoning over time
