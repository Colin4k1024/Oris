#![allow(dead_code)]
//! # oris
//!
//! Programmable AI execution runtime in Rust: stateful graphs, agents, tools, RAG, and multi-step execution.
//!
//! ## Overview
//!
//! - **Chains** — LLM chains, conversational and sequential chains, Q&A, SQL
//! - **Agents** — Chat agents with tools, multi-agent (router, subagents, skills)
//! - **RAG** — Retrieval-augmented generation (agentic, hybrid, two-step)
//! - **Graph** — State graphs, streaming, persistence, interrupts, subgraphs
//! - **Deep Agent** — Planning, filesystem tools, skills, long-term memory, human-in-the-loop
//! - **Vector stores** — PostgreSQL (pgvector), Qdrant, SQLite (VSS/Vec), SurrealDB, OpenSearch, Chroma, FAISS, MongoDB, Pinecone, Weaviate (enable via features)
//! - **Embeddings** — OpenAI, Azure, Ollama, FastEmbed, Mistral (feature-gated)
//! - **Document loaders** — PDF, HTML, CSV, Git, code, and more (feature-gated)
//!
//! ## Installation
//!
//! ```toml
//! [dependencies]
//! oris = "5"
//! # With a vector store, e.g. PostgreSQL:
//! # oris = { version = "5", features = ["postgres"] }
//! ```
//!
//! ## Example
//!
//! ```ignore
//! use oris_runtime::chain::{Chain, LLMChainBuilder};
//! use oris_runtime::llm::openai::OpenAI;
//! use oris_runtime::prompt::HumanMessagePromptTemplate;
//! use oris_runtime::prompt::prompt_args;
//!
//! # async fn run() -> Result<(), Box<dyn std::error::Error>> {
//! let llm = OpenAI::default();
//! let prompt = HumanMessagePromptTemplate::new("Hello, {name}!".into());
//! let chain = LLMChainBuilder::new().prompt(prompt).llm(llm).build()?;
//! let out = chain.invoke(prompt_args! { "name" => "Rust" }).await?;
//! # Ok(()) }
//! ```
//!
//! See the [repository](https://github.com/Colin4k1024/Oris) and [examples](https://github.com/Colin4k1024/Oris/tree/main/examples) for more.
//!
//! ## Stable API (0.1.x)
//!
//! The following modules are the **stable surface**; we avoid breaking changes to their public paths in 0.1.x:
//!
//! - **[graph](graph)** — State graphs, execution, persistence, interrupts. Use `graph::StateGraph`, `graph::MessagesState`, checkpointer, and interrupt/resume.
//! - **[agent](agent)** — Agent loop, tools, Deep Agent (planning, skills).
//! - **[tools](tools)** — Tool trait and built-in tools.
//!
//! State types (e.g. `graph::MessagesState`) are part of the stable graph API. Other modules (chain, document_loaders, llm, rag, etc.) are building blocks and may see path or API adjustments in minor updates.

/// Agents: conversational and unified agents, tools, executor, middleware, Deep Agent. **Stable API.**
pub mod agent;
/// Chains: LLM, conversational, sequential, QA, SQL, RAG chains and options. Experimental API in 0.1.x.
pub mod chain;
/// Document loaders: PDF, HTML, CSV, Git, S3, and more (feature-gated). Experimental API in 0.1.x.
pub mod document_loaders;
/// Embedding models (OpenAI, Ollama, FastEmbed, etc.; feature-gated). Experimental API in 0.1.x.
pub mod embedding;
/// Unified error types and utilities.
pub mod error;
/// Graph: state graphs, streaming, persistence, subgraphs, interrupts. **Stable API.**
pub mod graph;
/// Kernel API (2.0): event log, snapshot, reducer, action, step, policy, driver. Experimental API in 0.1.x.
pub mod kernel;
/// Common LLM/embedding traits and config. Experimental API in 0.1.x.
pub mod language_models;
/// LLM implementations: OpenAI, Claude, Ollama, Mistral, etc. (feature-gated). Experimental API in 0.1.x.
pub mod llm;
/// Memory: simple, conversational, and long-term (Deep Agent). Experimental API in 0.1.x.
pub mod memory;
/// Output parsers for chains and agents. Experimental API in 0.1.x.
pub mod output_parsers;
/// Prompts, templates, and message formatting. Experimental API in 0.1.x.
pub mod prompt;
/// RAG: agentic, hybrid, and two-step retrieval-augmented generation. Experimental API in 0.1.x.
pub mod rag;
/// Retrievers and rerankers (feature-gated). Experimental API in 0.1.x.
pub mod retrievers;
/// Schemas: messages, documents, prompts, memory. Experimental API in 0.1.x.
pub mod schemas;
/// Semantic routing and routing layers. Experimental API in 0.1.x.
pub mod semantic_router;
/// Text splitters and code splitters (tree-sitter when enabled). Experimental API in 0.1.x.
pub mod text_splitter;
/// Tools: command, search, Wolfram, long-term memory, etc. **Stable API.**
pub mod tools;
/// Utilities: similarity, vectors, builder, async helpers. Experimental API in 0.1.x.
pub mod utils;
/// Vector stores: pgvector, Qdrant, SQLite, SurrealDB, etc. (feature-gated). Experimental API in 0.1.x.
pub mod vectorstore;

pub use url;

// ============================================================================
// Type Aliases for Common Type Combinations
// ============================================================================

use std::sync::Arc;
use tokio::sync::Mutex;

/// Type alias for a tool wrapped in Arc
pub type Tool = Arc<dyn crate::tools::Tool>;

/// Type alias for a list of tools
pub type Tools = Vec<Arc<dyn crate::tools::Tool>>;

/// Type alias for tool context
pub type ToolContext = Arc<dyn crate::tools::ToolContext>;

/// Type alias for tool store
pub type ToolStore = Arc<dyn crate::tools::ToolStore>;

/// Type alias for agent state
pub type AgentState = Arc<Mutex<crate::agent::AgentState>>;

/// Type alias for memory
pub type Memory = Arc<Mutex<dyn crate::schemas::memory::BaseMemory>>;

/// Type alias for middleware list
pub type MiddlewareList = Vec<Arc<dyn crate::agent::Middleware>>;

/// Type alias for message list
pub type Messages = Vec<crate::schemas::Message>;

/// Type alias for embedding vector (f64)
pub type Embedding = Vec<f64>;

/// Type alias for embedding vector (f32)
pub type EmbeddingF32 = Vec<f32>;

/// Type alias for document list
pub type Documents = Vec<crate::schemas::Document>;
