use async_trait::async_trait;
use thiserror::Error;

use crate::tools::store::ToolStore;

use super::{StoreFilter, StoreValue};

/// Error types for enhanced store operations
#[derive(Error, Debug)]
pub enum StoreError {
    #[error("Store operation error: {0}")]
    OperationError(String),

    #[error("Vector index error: {0}")]
    VectorIndexError(String),

    #[error("Filter error: {0}")]
    FilterError(String),

    #[error("Embedding error: {0}")]
    EmbeddingError(String),
}

/// Enhanced store trait that extends ToolStore with vector search and metadata support.
///
/// This trait provides additional functionality for long-term memory:
/// - Metadata support (StoreValue with metadata)
/// - Vector similarity search
/// - Content filtering
#[async_trait]
pub trait EnhancedToolStore: ToolStore {
    /// Get a value with its metadata from the store
    async fn get_with_metadata(&self, namespace: &[&str], key: &str) -> Option<StoreValue>;

    /// Put a value with metadata into the store
    async fn put_with_metadata(&self, namespace: &[&str], key: &str, value: StoreValue);

    /// Search for values using vector similarity search and optional filters
    ///
    /// # Arguments
    /// * `namespace` - Namespace to search in
    /// * `query` - Optional text query for vector similarity search
    /// * `filter` - Optional filter to apply to results
    /// * `limit` - Maximum number of results to return
    ///
    /// # Returns
    /// Vector of StoreValue results, sorted by relevance if query is provided
    async fn search(
        &self,
        namespace: &[&str],
        query: Option<&str>,
        filter: Option<&StoreFilter>,
        limit: usize,
    ) -> Result<Vec<StoreValue>, StoreError>;

    /// Search for values using only filters (no vector search)
    ///
    /// This is useful when you want to filter by content or metadata
    /// without performing vector similarity search.
    async fn search_by_filter(
        &self,
        namespace: &[&str],
        filter: &StoreFilter,
        limit: usize,
    ) -> Result<Vec<StoreValue>, StoreError> {
        self.search(namespace, None, Some(filter), limit).await
    }
}
