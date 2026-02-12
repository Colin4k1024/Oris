use thiserror::Error;

use crate::chain::ChainError;

/// RAG-specific error types
#[derive(Error, Debug)]
pub enum RAGError {
    #[error("Chain error: {0}")]
    ChainError(#[from] ChainError),

    #[error("Retriever error: {0}")]
    RetrieverError(String),

    #[error("Query enhancement error: {0}")]
    QueryEnhancementError(String),

    #[error("Retrieval validation error: {0}")]
    RetrievalValidationError(String),

    #[error("Answer validation error: {0}")]
    AnswerValidationError(String),

    #[error("Invalid configuration: {0}")]
    InvalidConfiguration(String),

    #[error("Serde json error: {0}")]
    SerdeJsonError(#[from] serde_json::Error),
}

pub mod agentic;
pub mod hybrid;
pub mod two_step;

// Re-export commonly used types
pub use agentic::{AgenticRAG, AgenticRAGBuilder, RetrieverInfo, RetrieverTool};
pub use hybrid::{
    AnswerValidator, HybridRAG, HybridRAGBuilder, HybridRAGConfig, KeywordQueryEnhancer,
    LLMAnswerValidator, LLMQueryEnhancer, LLMRetrievalValidator, QueryEnhancer, RelevanceValidator,
    RetrievalValidator, SourceAlignmentValidator,
};
pub use two_step::{TwoStepRAG, TwoStepRAGBuilder};
