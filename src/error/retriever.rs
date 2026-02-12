use thiserror::Error;

use crate::error::VectorStoreError;

/// Retriever 相关的所有错误类型
#[derive(Error, Debug)]
pub enum RetrieverError {
    // ============ 基础检索错误 ============
    #[error("Query failed: {0}")]
    QueryError(String),

    #[error("Document processing error: {0}")]
    DocumentProcessingError(String),

    #[error("Vector store error: {0}")]
    VectorStoreError(#[from] VectorStoreError),

    // ============ 配置错误 ============
    #[error("Retriever configuration error: {0}")]
    ConfigurationError(String),

    #[error("Missing required configuration: {0}")]
    MissingConfiguration(String),

    // ============ 外部 API 错误 ============
    #[error("Wikipedia API error: {0}")]
    WikipediaError(String),

    #[error("arXiv API error: {0}")]
    ArxivError(String),

    #[error("Tavily API error: {0}")]
    TavilyError(String),

    #[error("Remote API error: {0}")]
    RemoteAPIError(String),

    // ============ 算法错误 ============
    #[error("BM25 indexing error: {0}")]
    BM25Error(String),

    #[error("TF-IDF calculation error: {0}")]
    TFIDFError(String),

    #[error("SVM error: {0}")]
    SVMError(String),

    #[error("Reranker error: {0}")]
    RerankerError(String),

    // ============ 集合和索引错误 ============
    #[error("Index not found: {0}")]
    IndexNotFoundError(String),

    #[error("Collection not found: {0}")]
    CollectionNotFound(String),

    // ============ 速率限制和超时错误 ============
    #[error("Rate limit exceeded")]
    RateLimitError,

    #[error("Timeout: {0}")]
    TimeoutError(String),

    // ============ 参数验证错误 ============
    #[error("Invalid query: {0}")]
    InvalidQuery(String),

    #[error("Invalid parameter: {0}")]
    InvalidParameter(String),

    // ============ 内部错误 ============
    #[error("Internal error: {0}")]
    InternalError(String),

    #[error("Unknown error: {0}")]
    Unknown(String),
}

impl From<String> for RetrieverError {
    fn from(s: String) -> Self {
        RetrieverError::Unknown(s)
    }
}

impl From<crate::error::VectorStoreError> for RetrieverError {
    fn from(e: crate::error::VectorStoreError) -> Self {
        RetrieverError::InternalError(format!("Vector store error: {}", e))
    }
}
