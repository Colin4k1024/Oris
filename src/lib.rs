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
//! - **LangGraph** — State graphs, streaming, persistence, interrupts, subgraphs
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
//! use oris::chain::{Chain, LLMChainBuilder};
//! use oris::llm::openai::OpenAI;
//! use oris::prompt::HumanMessagePromptTemplate;
//! use oris::prompt::prompt_args;
//!
//! # async fn run() -> Result<(), Box<dyn std::error::Error>> {
//! let llm = OpenAI::default();
//! let prompt = HumanMessagePromptTemplate::new("Hello, {name}!".into());
//! let chain = LLMChainBuilder::new().prompt(prompt).llm(llm).build()?;
//! let out = chain.invoke(prompt_args! { "name" => "Rust" }).await?;
//! # Ok(()) }
//! ```
//!
//! See the [repository](https://github.com/fanjia1024/oris) and [examples](https://github.com/fanjia1024/oris/tree/main/examples) for more.

/// Agents: conversational and unified agents, tools, executor, middleware, Deep Agent.
pub mod agent;
/// Chains: LLM, conversational, sequential, QA, SQL, RAG chains and options.
pub mod chain;
/// Document loaders: PDF, HTML, CSV, Git, S3, and more (feature-gated).
pub mod document_loaders;
/// Embedding models (OpenAI, Ollama, FastEmbed, etc.; feature-gated).
pub mod embedding;
/// Unified error types and utilities.
pub mod error;
/// LangGraph: state graphs, streaming, persistence, subgraphs.
pub mod langgraph;
/// Common LLM/embedding traits and config.
pub mod language_models;
/// LLM implementations: OpenAI, Claude, Ollama, Mistral, etc. (feature-gated).
pub mod llm;
/// Memory: simple, conversational, and long-term (Deep Agent).
pub mod memory;
/// Output parsers for chains and agents.
pub mod output_parsers;
/// Prompts, templates, and message formatting.
pub mod prompt;
/// RAG: agentic, hybrid, and two-step retrieval-augmented generation.
pub mod rag;
/// Retrievers and rerankers (feature-gated).
pub mod retrievers;
/// Schemas: messages, documents, prompts, memory.
pub mod schemas;
/// Semantic routing and routing layers.
pub mod semantic_router;
/// Text splitters and code splitters (tree-sitter when enabled).
pub mod text_splitter;
/// Tools: command, search, Wolfram, long-term memory, etc.
pub mod tools;
/// Utilities: similarity, vectors, builder, async helpers.
pub mod utils;
/// Vector stores: pgvector, Qdrant, SQLite, SurrealDB, etc. (feature-gated).
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
