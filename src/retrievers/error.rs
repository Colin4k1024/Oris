use thiserror::Error;

use crate::vectorstore::VectorStoreError;

/// Errors specific to retrievers
#[derive(Error, Debug)]
pub enum RetrieverError {
    #[error("Wikipedia API error: {0}")]
    WikipediaError(String),

    #[error("arXiv API error: {0}")]
    ArxivError(String),

    #[error("Tavily API error: {0}")]
    TavilyError(String),

    #[error("BM25 indexing error: {0}")]
    BM25Error(String),

    #[error("TF-IDF calculation error: {0}")]
    TFIDFError(String),

    #[error("SVM error: {0}")]
    SVMError(String),

    #[error("Reranker error: {0}")]
    RerankerError(String),

    #[error("Retriever configuration error: {0}")]
    ConfigurationError(String),

    #[error("Document processing error: {0}")]
    DocumentProcessingError(String),

    #[error("Vector store error: {0}")]
    VectorStoreError(#[from] VectorStoreError),

    #[error("Unknown error: {0}")]
    Unknown(String),
}

impl From<Box<dyn std::error::Error + Send + Sync>> for RetrieverError {
    fn from(e: Box<dyn std::error::Error + Send + Sync>) -> Self {
        RetrieverError::Unknown(e.to_string())
    }
}
