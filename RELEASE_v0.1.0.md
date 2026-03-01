# v0.1.0 — First crates.io release

First stable release of **Oris**: a programmable execution runtime for AI agents in Rust.

Oris is not a chat SDK or a prompt library. It is a **runtime** for long-running, durable agent workflows: stateful graphs, checkpoints, interrupts, and recovery. Think Temporal/Cadence for reasoning processes.

## What's in this release

- **State graphs** — Define workflows as directed graphs; run, stream, and persist state (in-memory or SQLite).
- **Durable execution** — Checkpoint state, resume runs, and survive process restarts.
- **Human-in-the-loop** — Pause for approval or review, then resume with decisions.
- **Agents and tools** — Chat agents with tools; optional multi-agent (router, subagents) and Deep Agent (planning, filesystem, skills).
- **RAG, chains, vector stores** — Retrieval-augmented generation, LLM chains, and optional vector stores (PostgreSQL, Qdrant, SQLite, SurrealDB, etc.) behind features.

## Install

```bash
cargo add oris-runtime
# With a vector store, e.g. PostgreSQL:
cargo add oris-runtime --features postgres
```

## Links

- **Crate:** [crates.io/crates/oris-runtime](https://crates.io/crates/oris-runtime)
- **Docs:** [docs.rs/oris-runtime](https://docs.rs/oris-runtime)
- **Repo:** [github.com/Colin4k1024/Oris](https://github.com/Colin4k1024/Oris)
- **Examples:** [examples](https://github.com/Colin4k1024/Oris/tree/main/examples)

## Note

This is an early release. The runtime, graph execution, and agent loop are implemented and usable. We are stabilizing the public API surface (see README) and will add a **durable long-running job** example next to showcase interrupt, resume, and persistence in one flow.
