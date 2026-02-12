use std::collections::HashMap;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::error::PersistenceError;

/// Store item - represents a value stored in the store
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StoreItem {
    /// The stored value
    pub value: Value,
    /// Unique key for this item in the namespace
    pub key: String,
    /// Namespace tuple
    pub namespace: Vec<String>,
    /// Timestamp when this item was created
    pub created_at: DateTime<Utc>,
    /// Timestamp when this item was last updated
    pub updated_at: DateTime<Utc>,
}

impl StoreItem {
    /// Create a new StoreItem
    pub fn new(value: Value, key: String, namespace: Vec<String>) -> Self {
        let now = Utc::now();
        Self {
            value,
            key,
            namespace,
            created_at: now,
            updated_at: now,
        }
    }
}

/// Trait for cross-thread storage
///
/// Stores allow sharing information across threads (conversations).
/// This is useful for maintaining user-specific memories or other
/// persistent data that should be accessible across multiple threads.
#[async_trait]
pub trait Store: Send + Sync {
    /// Put a value into the store
    async fn put(
        &self,
        namespace: &[&str],
        key: &str,
        value: Value,
    ) -> Result<(), PersistenceError>;

    /// Get a value from the store
    async fn get(
        &self,
        namespace: &[&str],
        key: &str,
    ) -> Result<Option<StoreItem>, PersistenceError>;

    /// Search for items in the store
    ///
    /// If query is None, returns all items in the namespace.
    /// If query is Some, performs semantic search (if supported).
    async fn search(
        &self,
        namespace: &[&str],
        query: Option<&str>,
        limit: Option<usize>,
    ) -> Result<Vec<StoreItem>, PersistenceError>;

    /// Delete a value from the store
    async fn delete(&self, namespace: &[&str], key: &str) -> Result<(), PersistenceError>;

    /// Check if semantic search is enabled
    ///
    /// Returns true if this store implementation supports semantic search
    /// using embeddings. Default implementation returns false.
    fn supports_semantic_search(&self) -> bool {
        false
    }

    /// Get embedding dimensions (if semantic search is enabled)
    ///
    /// Returns the dimension of embeddings used for semantic search.
    /// Returns None if semantic search is not supported.
    fn embedding_dims(&self) -> Option<usize> {
        None
    }
}

/// In-memory store implementation
///
/// This is useful for development and testing. Items are stored
/// in memory and will be lost when the process exits.
pub struct InMemoryStore {
    data: tokio::sync::RwLock<HashMap<String, StoreItem>>,
}

impl InMemoryStore {
    /// Create a new InMemoryStore
    pub fn new() -> Self {
        Self {
            data: tokio::sync::RwLock::new(HashMap::new()),
        }
    }

    /// Make a key from namespace and key
    fn make_key(namespace: &[&str], key: &str) -> String {
        if namespace.is_empty() {
            key.to_string()
        } else {
            format!("{}:{}", namespace.join(":"), key)
        }
    }
}

impl Default for InMemoryStore {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Store for InMemoryStore {
    async fn put(
        &self,
        namespace: &[&str],
        key: &str,
        value: Value,
    ) -> Result<(), PersistenceError> {
        let store_key = Self::make_key(namespace, key);
        let namespace_vec: Vec<String> = namespace.iter().map(|s| s.to_string()).collect();

        let item = StoreItem::new(value, key.to_string(), namespace_vec);

        let mut data = self.data.write().await;
        data.insert(store_key, item);

        Ok(())
    }

    async fn get(
        &self,
        namespace: &[&str],
        key: &str,
    ) -> Result<Option<StoreItem>, PersistenceError> {
        let store_key = Self::make_key(namespace, key);
        let data = self.data.read().await;
        Ok(data.get(&store_key).cloned())
    }

    async fn search(
        &self,
        namespace: &[&str],
        query: Option<&str>,
        limit: Option<usize>,
    ) -> Result<Vec<StoreItem>, PersistenceError> {
        let namespace_prefix = if namespace.is_empty() {
            String::new()
        } else {
            format!("{}:", namespace.join(":"))
        };

        let data = self.data.read().await;
        let mut results: Vec<StoreItem> = data
            .values()
            .filter(|item| {
                // Filter by namespace
                if !namespace_prefix.is_empty() {
                    let item_prefix = format!("{}:", item.namespace.join(":"));
                    if item_prefix != namespace_prefix {
                        return false;
                    }
                } else if !item.namespace.is_empty() {
                    return false;
                }

                // If query is provided, do simple string matching
                // (semantic search would require embeddings)
                if let Some(q) = query {
                    let value_str = serde_json::to_string(&item.value).unwrap_or_default();
                    value_str.to_lowercase().contains(&q.to_lowercase())
                } else {
                    true
                }
            })
            .cloned()
            .collect();

        // Sort by created_at (most recent first)
        results.sort_by(|a, b| b.created_at.cmp(&a.created_at));

        // Apply limit
        if let Some(limit) = limit {
            results.truncate(limit);
        }

        Ok(results)
    }

    async fn delete(&self, namespace: &[&str], key: &str) -> Result<(), PersistenceError> {
        let store_key = Self::make_key(namespace, key);
        let mut data = self.data.write().await;
        data.remove(&store_key);
        Ok(())
    }
}

/// Type alias for a boxed store
pub type StoreBox = std::sync::Arc<dyn Store>;

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_in_memory_store() {
        let store = InMemoryStore::new();

        // Put a value
        store
            .put(
                &["user-1", "memories"],
                "memory-1",
                serde_json::json!({"food_preference": "I like pizza"}),
            )
            .await
            .unwrap();

        // Get the value
        let item = store
            .get(&["user-1", "memories"], "memory-1")
            .await
            .unwrap();
        assert!(item.is_some());
        assert_eq!(
            item.unwrap().value,
            serde_json::json!({"food_preference": "I like pizza"})
        );

        // Search
        let results = store
            .search(&["user-1", "memories"], Some("pizza"), None)
            .await
            .unwrap();
        assert_eq!(results.len(), 1);

        // Delete
        store
            .delete(&["user-1", "memories"], "memory-1")
            .await
            .unwrap();
        let item = store
            .get(&["user-1", "memories"], "memory-1")
            .await
            .unwrap();
        assert!(item.is_none());
    }
}
