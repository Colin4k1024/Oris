# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Oris is a **self-evolving execution runtime** — software that reasons, learns, and improves itself. It combines durable execution with self-evolution capabilities where software can:
- **Detect** problems from runtime signals (compiler diagnostics, panics, test failures)
- **Select** the best candidate gene for solving the problem
- **Mutate** generate solutions based on successful patterns
- **Execute** sandboxed mutations safely
- **Validate** ensure correctness before promotion
- **Evaluate** measure improvement vs regression
- **Solidify** promote successful mutations to reusable genes
- **Reuse** apply proven solutions from the gene pool

> **North-Star Outcome:**
> Task → Detect → Replay if trusted → Mutate only when needed → Validate → Capture → Reuse → Reduce reasoning over time

**Current version:** oris-runtime v0.61.0 · 747 Rust source files · ~185,000 LOC · 295 unit tests · 50+ feature flags

## Workspace Structure

This is a Cargo workspace with 16 library crates and 6 example projects.

### Library Crates (`crates/`)

| Crate | Version | Purpose |
|-------|---------|---------|
| **oris-runtime** | 0.61.0 | Main crate: agentic workflow runtime, graphs, agents, tools, RAG, multi-step execution |
| **oris-kernel** | 0.2.13 | Deterministic execution kernel: event log, replay, snapshot, actions, policies |
| **oris-execution-runtime** | 0.3.0 | Control plane: scheduler, lease manager, repositories, circuit breaker, crash recovery |
| **oris-execution-server** | 0.2.12 | Graph-aware HTTP execution server facade |
| **oris-evokernel** | 0.14.1 | Self-evolving kernel orchestration (highest fan-in crate, depends on 11 others) |
| **oris-evolution** | 0.4.1 | Core: Gene, Capsule, EvolutionEvent, Selector, Pipeline, Confidence, Task Classes |
| **oris-sandbox** | 0.3.0 | Sandboxed mutation execution with OS-level resource isolation |
| **oris-governor** | 0.3.2 | Promotion, cooldown, and revocation policies |
| **oris-intake** | 0.4.0 | Automatic issue intake, deduplication, prioritization, webhook support |
| **oris-evolution-network** | 0.5.0 | OEN envelope, gossip sync, Ed25519 signing, rate limiting |
| **oris-economics** | 0.2.0 | Local EVU ledger and reputation accounting |
| **oris-spec** | 0.2.2 | OUSL YAML spec contracts and compilers |
| **oris-agent-contract** | 0.5.5 | External agent proposal contracts (proposal-only interface) |
| **oris-orchestrator** | 0.5.0 | Autonomous loop, release automation, GitHub delivery, task planning |
| **oris-mutation-evaluator** | 0.3.0 | Two-phase mutation quality evaluator (static analysis + LLM critic) |
| **oris-genestore** | 0.2.0 | SQLite-based Gene and Capsule storage |

### Example Projects (`examples/`)

| Example | Purpose |
|---------|---------|
| `evo_oris_repo` | Canonical evolution scenario + bins: `intake_webhook_demo`, `confidence_lifecycle_demo`, `network_exchange` |
| `oris_starter_axum` | Starter Axum integration (execution-server + a2a-production + mcp-experimental) |
| `oris_worker_tokio` | Worker process example (HTTP-based task polling) |
| `oris_operator_cli` | Operator CLI (clap-based) for managing runtime |
| `plugin_reference` | Reference layout for external graph node plugins |
| `vector_store_surrealdb` | SurrealDB vector store example |

### Dependency Graph (Clean DAG)

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

Layer 3 (highest fan-in):
  oris-evokernel → 11 crates
  oris-runtime → oris-evokernel (optional), oris-execution-runtime, oris-kernel

Layer 4:
  oris-execution-server → oris-runtime
```

## Common Commands

### Building
```bash
# Build the workspace
cargo build

# Build with all features (required for full validation)
cargo build --all --release --all-features
```

### Testing
```bash
# Run all tests
cargo test --release --all-features

# Run targeted tests for a specific crate
cargo test -p oris-runtime <test_name_or_module>

# Run evolution feature wiring test
cargo test -p oris-runtime --test evolution_feature_wiring --features full-evolution-experimental

# Format check
cargo fmt --all -- --check
```

### Running Examples
```bash
# Run the execution server (HTTP API for jobs)
cargo run -p oris-runtime --example execution_server --features "sqlite-persistence,execution-server"

# Run canonical evolution example
cargo run -p evo_oris_repo

# Run evolution bins
cargo run -p evo_oris_repo --bin intake_webhook_demo
cargo run -p evo_oris_repo --bin confidence_lifecycle_demo
cargo run -p evo_oris_repo --bin network_exchange

# Axum starter
cargo run -p oris_starter_axum

# Operator CLI
cargo run -p oris_operator_cli
```

### Linting
```bash
cargo fmt --all
```

### Project Statistics (LOC & Git Contributions)
```bash
# Count total lines of code (Rust + TOML + Markdown) and all-author git stats
./scripts/count_loc.sh

# Show git contribution stats for a specific author
./scripts/count_loc.sh "Colin4k1024"
```

### Publishing
```bash
# Dry-run before real publish
cargo publish -p oris-runtime --all-features --dry-run
```

## Architecture

### Core Modules (`crates/oris-runtime/src/`)

| Module | Purpose | Stability |
|--------|---------|-----------|
| **graph/** | State graphs, compiled graph, edges, nodes, persistence, interrupts, streaming, plugins, tasks, trace | Stable API |
| **agent/** | Agent trait, chat agents, unified agent, deep agent (planning, filesystem, skills, HITL), multi-agent (router, subagents, handoffs), context engineering, middleware, structured output | Stable API |
| **tools/** | Tool trait, command executor, DuckDuckGo, SerpAPI, SQL, scraper, browser-use, Wolfram, long-term memory tool, sequential thinking, TTS, tool store | Stable API |
| **llm/** | OpenAI, Claude, Ollama, Mistral, Gemini, Bedrock, DeepSeek, Qwen, HuggingFace | Experimental |
| **memory/** | Simple, conversational, long-term memory | Experimental |
| **vectorstore/** | pgvector, Qdrant, SQLite VSS/Vec, SurrealDB, OpenSearch, Chroma, FAISS, MongoDB, Pinecone, Weaviate, in-memory | Experimental |
| **document_loaders/** | PDF, HTML, CSV, Git, S3 loaders | Experimental |
| **rag/** | Agentic, hybrid, two-step RAG with query enhancement, reranker, compression | Experimental |
| **retrievers/** | Algorithm-based, external, hybrid retrievers with reranking | Experimental |
| **chain/** | LLM chains, conversational, sequential, QA, SQL, RAG chains | Experimental |
| **embedding/** | OpenAI, Azure, Ollama, FastEmbed, Mistral embeddings | Experimental |
| **text_splitter/** | Text and code splitters (tree-sitter-based) | Experimental |
| **plugins.rs** | K4 plugin system: 9 categories (Node, Tool, Memory, LLMAdapter, Scheduler, Checkpoint, Effect, Observer, Governor), determinism contracts, resource limits, version negotiation, PluginRegistry | Experimental |
| **execution_server/** | Graph-aware HTTP execution server (api_handlers, benchmark_suite, graph_bridge) | Experimental |
| **evolution.rs** | Re-exports oris-evokernel (full evolution layer) | Experimental |
| **semantic_router/** | Semantic routing layers | Experimental |

### Kernel Module Structure (`oris-kernel/src/kernel/`)

36 sub-modules organized as:

- **Core**: action, event, event_store, state, step, stubs, identity (RunId, Seq, StepId)
- **Execution**: driver (Kernel, RunStatus, Signal), runner (KernelRunner), execution_step, execution_log, execution_suspension
- **Replay**: replay_cursor, replay_resume, replay_verifier, determinism_guard
- **Persistence**: snapshot (InMemorySnapshotStore, SnapshotStore), sqlite_store, postgres_store
- **Policy**: policy (AllowListPolicy, BudgetRules, RetryWithBackoffPolicy), kernel_mode
- **Interrupts**: interrupt, interrupt_resolver, kernel_interrupt (state machine)
- **Advanced**: reducer, runtime_effect (EffectSink), timeline, timeline_fork

### Key Abstractions

**StateGraph** (`graph/graph.rs`): Builder for creating stateful graphs with nodes, edges, and conditional routing.

**CompiledGraph** (`graph/compiled.rs`): Executable representation with `invoke()`, `stream()`, `step_once()` methods.

**Checkpointer** (`graph/persistence/checkpointer.rs`): Trait for checkpointing state. Implementations: `InMemorySaver`, `SqliteCheckpointer`.

**Agent** (`agent/agent.rs`): Trait for building agents with `plan()` and `get_tools()` methods.

**Tool** (`tools/tool.rs`): Trait for implementing tools that agents can call.

**Kernel** (`oris-kernel driver.rs`): Deterministic execution kernel with event log, replay, and snapshot support.

**PluginRegistry** (`plugins.rs`): Registry for 9 plugin categories with determinism contracts, resource limits, and version negotiation.

**SkeletonScheduler** (`oris-execution-runtime scheduler.rs`): Context-aware scheduler with weighted priority dispatch and backpressure.

**CircuitBreaker** (`oris-execution-runtime`): Circuit breaker pattern for fault tolerance.

**EvolutionPipeline** (`oris-evolution pipeline.rs`): Detect → Select → Mutate → Execute → Validate → Solidify pipeline.

**MutationEvaluator** (`oris-mutation-evaluator`): Two-phase quality evaluator (static analysis + LLM critic).

**SqliteGeneStore** (`oris-genestore`): SQLite-based Gene and Capsule persistence.

### Stable API Surface

The public stable surface for building on Oris:
- `oris_runtime::graph` — State graphs, execution, persistence, interrupts, trace
- `oris_runtime::agent` — Agent loop, tools, Deep Agent, multi-agent patterns
- `oris_runtime::tools` — Tool trait and built-in tools

### Kernel Phases (K1–K5, all complete)

- **K1**: ExecutionStep contract freeze, effect capture, determinism guard
- **K2**: Canonical log store, replay cursor, replay verification, branch replay
- **K3**: Interrupt object, suspension state machine, replay-based resume
- **K4**: Plugin categories, determinism declarations, execution sandbox, version negotiation
- **K5**: Lease-based finalization, zero-data-loss recovery, context-aware scheduler, backpressure

## Feature Flags

### Persistence & Database
- `sqlite-persistence` — SQLite checkpointing (rusqlite)
- `postgres` — PostgreSQL via pgvector + sqlx
- `kernel-postgres` — PostgreSQL backend for kernel

### Vector Stores
- `surrealdb`, `qdrant`, `chroma`, `faiss`, `milvus`, `mongodb`, `pinecone`, `weaviate`
- `sqlite-vss`, `sqlite-vec` — SQLite vector search variants
- `opensearch` — AWS OpenSearch
- `in-memory` — In-memory vector store

### LLM Providers
- `ollama` — Ollama local LLM
- `mistralai` — Mistral AI
- `gemini` — Google Gemini
- `bedrock` — AWS Bedrock

### Document Loaders
- `yaml`, `toml`, `xml`, `excel` — Format-specific loaders
- `lopdf`, `pdf-extract` — PDF support
- `git` — Git repository loader
- `aws-s3` — S3 loader
- `github` — GitHub loader (octocrab)

### Retrieval & Embeddings
- `fastembed` — FastEmbed local embeddings
- `flashrank` — FlashRank reranker
- `wikipedia`, `arxiv`, `tavily`, `bm25`, `tfidf`, `svm`, `cohere`, `contextual-ai`, `llmlingua`

### Code Analysis
- `tree-sitter` — Code splitter (11 language parsers)

### Server & API
- `execution-server` — HTTP API server (axum)
- `mcp-experimental` — MCP bootstrap (requires execution-server)
- `a2a-production` — Production A2A protocol boundary

### Tools
- `html-to-markdown` — htmd integration
- `browser-use` — Headless Chrome browser automation

### Evolution (Experimental)
- `evokernel-facade` — Base: re-exports oris-evokernel
- `evolution-experimental` — Core evolution (includes evokernel-facade)
- `governor-experimental` — Governor policies
- `evolution-network-experimental` — Evolution network
- `economics-experimental` — EVU ledger
- `spec-experimental` — OUSL spec contracts
- `agent-contract-experimental` — Agent proposal contracts
- `full-evolution-experimental` — Aggregate: all experimental features above

### Sub-crate Feature Flags (not exposed through oris-runtime)
- `oris-sandbox`: `resource-limits` — OS-level process isolation
- `oris-evolution-network`: `gossip-dns`, `gossip-msgpack`, `network-mtls`
- `oris-intake`: `webhook` — Axum webhook endpoints with HMAC verification
- `oris-mutation-evaluator`: `llm-http` — Real HTTP calls for LLM backends
- `oris-orchestrator`: `release-automation-experimental` — Autonomous release executor
- `oris-evolution`: `evolution-experimental` — TOML-based task class loading

## Development Workflow

This repository follows an **issue-driven GitHub workflow**. When working on maintenance or features:

1. **Preflight**: Check `git status --short --branch`, verify `gh auth status`, list open issues with `gh issue list --state open --limit 20`

2. **Issue Selection**: Use `gh issue view <number>` to read issue details. Apply selection order from `skills/oris-maintainer/references/issue-selection.md`

3. **Implementation**: Make narrow, issue-scoped changes. Avoid opportunistic refactors.

4. **Validation**: Run in order:
   - `cargo fmt --all`
   - `cargo test -p oris-runtime <targeted_test>`
   - Full validation: `cargo fmt --all -- --check && cargo build --all --release --all-features && cargo test --release --all-features`

5. **Release**: Use `cargo publish -p oris-runtime --all-features --dry-run` before real publish

See `skills/oris-maintainer/SKILL.md` and `skills/oris-maintainer/references/command-checklist.md` for the full workflow.

**Cursor Rules**: When working in Cursor, see `.cursor/rules/oris-maintainer.mdc` for maintainer process guidelines.

## Key Documentation

| Document | Location |
|----------|----------|
| Architecture overview | `docs/ARCHITECTURE.md` |
| Q1 2026 project audit | `docs/PROJECT_AUDIT_2026_Q1.md` |
| Release history | `RELEASE.md` |
| Plugin authoring guide | `docs/plugin-authoring.md` |
| Kernel API reference | `docs/kernel-api.md` |
| Oris 2.0 strategy | `docs/ORIS_2.0_STRATEGY.md` |
| Self-evolution boundary | `docs/evolution-boundary.md` |
| Production operations | `docs/production-operations-guide.md` |
| Incident response | `docs/incident-response-runbook.md` |
| Schema migrations | `docs/runtime-schema-migrations.md` |

## Environment Variables

| Variable | Purpose |
|----------|---------|
| `OPENAI_API_KEY` | OpenAI API key |
| `ANTHROPIC_API_KEY` | Anthropic API key |
| `OLLAMA_HOST` | Ollama host (default `http://localhost:11434`) |
| `ORIS_SERVER_ADDR` | Execution server address (default `127.0.0.1:8080`) |
| `ORIS_SQLITE_DB` | SQLite database path |
| `ORIS_RUNTIME_BACKEND` | Runtime backend (`sqlite` or `postgres`) |
