//! Reranker retrievers
//!
//! These retrievers rerank documents from other retrievers to improve relevance.
//! They use external APIs or local models to score and reorder documents.

#[cfg(feature = "cohere")]
mod cohere_reranker;
#[cfg(feature = "cohere")]
pub use cohere_reranker::*;

#[cfg(feature = "flashrank")]
mod flashrank_reranker;
#[cfg(feature = "flashrank")]
pub use flashrank_reranker::*;

#[cfg(feature = "contextual-ai")]
mod contextual_ai_reranker;
#[cfg(feature = "contextual-ai")]
pub use contextual_ai_reranker::*;
