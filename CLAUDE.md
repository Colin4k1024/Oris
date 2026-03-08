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

The main crate is `oris-runtime` in `crates/oris-runtime/`. This is a Cargo workspace with examples in `examples/`.

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

# Run targeted tests for a specific module
cargo test -p oris-runtime <test_name_or_module>

# Format check
cargo fmt --all -- --check
```

### Running Examples
```bash
# Run the execution server (HTTP API for jobs)
cargo run -p oris-runtime --example execution_server --features "sqlite-persistence,execution-server"

# Run other examples (list available in crates/oris-runtime/examples/)
cargo run -p oris-runtime --example <example_name> --features "..."
```

### Linting
```bash
cargo fmt --all
```

## Architecture

### Core Modules (in `crates/oris-runtime/src/`)

| Directory | Purpose |
|-----------|---------|
| **graph/** | State graphs, execution engine, persistence/checkpointing, interrupts, streaming |
| **agent/** | Agent loop (conversational, unified, Deep agent), tools, middleware, multi-agent patterns |
| **kernel/** | Kernel API (2.0) - event-first execution, actions, replay, determinism verification |
| **tools/** | Tool trait and built-in tools (command, search, SQL, scraper, browser-use, etc.) |
| **llm/** | LLM implementations (OpenAI, Claude, Ollama, Mistral, Gemini, Bedrock) |
| **memory/** | Memory implementations (simple, conversational, long-term) |
| **vectorstore/** | Vector stores (pgvector, Qdrant, SQLite, SurrealDB, etc.) |
| **document_loaders/** | PDF, HTML, CSV, Git, S3 loaders |
| **rag/** | RAG implementations (agentic, hybrid, two-step) |
| **evolution/** | Self-evolution: Gene, Capsule, Selector, Pipeline, Confidence |
| **intake/** | Issue intake, deduplication, prioritization |
| **evokernel/** | Signal extraction from runtime diagnostics |

### Key Abstractions

**StateGraph** (`graph/graph.rs`): Builder for creating stateful graphs with nodes, edges, and conditional routing.

**CompiledGraph** (`graph/compiled.rs`): Executable representation of a compiled graph with `invoke()`, `stream()`, `step_once()` methods.

**Checkpointer** (`graph/persistence/checkpointer.rs`): Trait for checkpointing state. Implementations: `InMemorySaver`, `SqliteCheckpointer`.

**Agent** (`agent/agent.rs`): Trait for building agents with `plan()` and `get_tools()` methods.

**Tool** (`tools/tool.rs`): Trait for implementing tools that agents can call.

### Evolution Crates

| Crate | Purpose |
|-------|---------|
| `oris-evolution` | Core: Gene, Capsule, EvolutionEvent, Selector, Pipeline, Confidence |
| `oris-evokernel` | Signal extraction from runtime diagnostics |
| `oris-intake` | Issue intake, deduplication, prioritization |
| `oris-evolution-network` | A2A protocol for evolution agents |
| `oris-sandbox` | Safe mutation execution |

### Running Evolution Examples

```bash
# Run canonical evolution example
cargo run -p evo_oris_repo

# Run supervised dev loop
cargo run -p evo_oris_repo --bin supervised_devloop

# Run network exchange
cargo run -p evo_oris_repo --bin network_exchange

# Run evolution feature wiring test
cargo test -p oris-runtime --test evolution_feature_wiring --features full-evolution-experimental
```

### Stable API (0.1.x)

The public stable surface for building on Oris:
- `oris_runtime::graph` — State graphs, execution, persistence, interrupts, trace
- `oris_runtime::agent` — Agent loop, tools, Deep Agent
- `oris_runtime::tools` — Tool trait and built-in tools

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

## Feature Flags

Key features for common use cases:
- `sqlite-persistence` — SQLite checkpointing for durable execution
- `postgres` / `kernel-postgres` — PostgreSQL backend
- `ollama` — Local LLM support
- `execution-server` — HTTP API server
- `surrealdb`, `qdrant`, `chroma` — Vector stores
- `evolution-experimental` — Core `oris_runtime::evolution`
- `full-evolution-experimental` — End-to-end facade (evolution, governor, network, economics)
- `a2a-production` — Production `/a2a/*` runtime boundary

## Environment Variables

| Variable | Purpose |
|----------|---------|
| `OPENAI_API_KEY` | OpenAI API key |
| `ANTHROPIC_API_KEY` | Anthropic API key |
| `OLLAMA_HOST` | Ollama host (default `http://localhost:11434`) |
| `ORIS_SERVER_ADDR` | Execution server address (default `127.0.0.1:8080`) |
| `ORIS_SQLITE_DB` | SQLite database path |
| `ORIS_RUNTIME_BACKEND` | Runtime backend (`sqlite` or `postgres`) |
