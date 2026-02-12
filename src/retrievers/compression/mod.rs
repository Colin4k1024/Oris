//! Document compression retrievers
//!
//! These retrievers compress or filter documents to reduce redundancy and improve efficiency.

#[cfg(feature = "llmlingua")]
mod llm_lingua_compressor;
#[cfg(feature = "llmlingua")]
pub use llm_lingua_compressor::*;

mod embeddings_redundant_filter;
pub use embeddings_redundant_filter::*;
