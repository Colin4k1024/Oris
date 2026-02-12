use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;

use crate::{
    embedding::embedder_trait::Embedder,
    tools::{
        long_term_memory::{EnhancedToolStore, StoreError, StoreFilter, StoreValue},
        store::ToolStore,
    },
};

/// Configuration for EnhancedInMemoryStore
#[derive(Clone)]
pub struct EnhancedInMemoryStoreConfig {
    /// Whether to enable vector index for similarity search
    pub enable_vector_index: bool,
    /// Optional embedder for vector indexing
    pub embedder: Option<Arc<dyn Embedder>>,
    /// Vector dimensions (required if enable_vector_index is true)
    pub vector_dimensions: Option<usize>,
}

impl Default for EnhancedInMemoryStoreConfig {
    fn default() -> Self {
        Self {
            enable_vector_index: false,
            embedder: None,
            vector_dimensions: None,
        }
    }
}

impl EnhancedInMemoryStoreConfig {
    /// Create a new config
    pub fn new() -> Self {
        Self::default()
    }

    /// Enable vector index
    pub fn with_vector_index(mut self, embedder: Arc<dyn Embedder>, dimensions: usize) -> Self {
        self.enable_vector_index = true;
        self.embedder = Some(embedder);
        self.vector_dimensions = Some(dimensions);
        self
    }
}

/// Internal storage structure for enhanced store
#[derive(Clone)]
struct StoredItem {
    value: StoreValue,
    vector: Option<Vec<f32>>,
}

/// Enhanced in-memory store with vector search and metadata support
pub struct EnhancedInMemoryStore {
    /// Storage data: namespace -> key -> StoredItem
    data: Arc<tokio::sync::RwLock<HashMap<String, HashMap<String, StoredItem>>>>,
    /// Configuration
    config: EnhancedInMemoryStoreConfig,
}

impl EnhancedInMemoryStore {
    /// Create a new EnhancedInMemoryStore
    pub fn new() -> Self {
        Self {
            data: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            config: EnhancedInMemoryStoreConfig::default(),
        }
    }

    /// Create with configuration
    pub fn with_config(config: EnhancedInMemoryStoreConfig) -> Self {
        Self {
            data: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            config,
        }
    }

    /// Make a namespace key from namespace slice
    fn make_namespace_key(namespace: &[&str]) -> String {
        if namespace.is_empty() {
            "default".to_string()
        } else {
            namespace.join(":")
        }
    }

    /// Make a full key from namespace and key
    fn make_full_key(namespace: &[&str], key: &str) -> String {
        let ns_key = Self::make_namespace_key(namespace);
        format!("{}:{}", ns_key, key)
    }

    /// Compute cosine similarity between two vectors
    fn cosine_similarity(a: &[f32], b: &[f32]) -> f64 {
        crate::utils::cosine_similarity_f32(a, b)
    }

    /// Extract text from a JSON value for embedding
    fn extract_text_for_embedding(value: &Value) -> String {
        match value {
            Value::String(s) => s.clone(),
            Value::Object(map) => {
                // Try to extract text fields
                if let Some(text) = map.get("text").and_then(|v| v.as_str()) {
                    text.to_string()
                } else if let Some(content) = map.get("content").and_then(|v| v.as_str()) {
                    content.to_string()
                } else {
                    // Fallback: serialize the whole object
                    serde_json::to_string(value).unwrap_or_default()
                }
            }
            Value::Array(arr) => arr
                .iter()
                .filter_map(|v| v.as_str())
                .collect::<Vec<_>>()
                .join(" "),
            _ => serde_json::to_string(value).unwrap_or_default(),
        }
    }
}

impl Default for EnhancedInMemoryStore {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ToolStore for EnhancedInMemoryStore {
    async fn get(&self, namespace: &[&str], key: &str) -> Option<Value> {
        self.get_with_metadata(namespace, key)
            .await
            .map(|sv| sv.value)
    }

    async fn put(&self, namespace: &[&str], key: &str, value: Value) {
        let store_value = StoreValue::new(value);
        self.put_with_metadata(namespace, key, store_value).await;
    }

    async fn delete(&self, namespace: &[&str], key: &str) {
        let ns_key = Self::make_namespace_key(namespace);
        let mut data = self.data.write().await;
        if let Some(namespace_map) = data.get_mut(&ns_key) {
            namespace_map.remove(key);
        }
    }

    async fn list(&self, namespace: &[&str]) -> Vec<String> {
        let ns_key = Self::make_namespace_key(namespace);
        let data = self.data.read().await;
        if let Some(namespace_map) = data.get(&ns_key) {
            namespace_map.keys().cloned().collect()
        } else {
            Vec::new()
        }
    }
}

#[async_trait]
impl EnhancedToolStore for EnhancedInMemoryStore {
    async fn get_with_metadata(&self, namespace: &[&str], key: &str) -> Option<StoreValue> {
        let ns_key = Self::make_namespace_key(namespace);
        let data = self.data.read().await;
        data.get(&ns_key)
            .and_then(|namespace_map| namespace_map.get(key))
            .map(|item| item.value.clone())
    }

    async fn put_with_metadata(&self, namespace: &[&str], key: &str, value: StoreValue) {
        let ns_key = Self::make_namespace_key(namespace);
        let mut data = self.data.write().await;

        // Compute vector if vector index is enabled
        let vector = if self.config.enable_vector_index {
            if let Some(embedder) = &self.config.embedder {
                let text = Self::extract_text_for_embedding(&value.value);
                match embedder.embed_query(&text).await {
                    Ok(embeddings) => {
                        // Convert f64 to f32
                        Some(embeddings.into_iter().map(|x| x as f32).collect())
                    }
                    Err(_) => None, // Silently fail if embedding fails
                }
            } else {
                None
            }
        } else {
            None
        };

        let item = StoredItem {
            value: value.clone(),
            vector,
        };

        let namespace_map = data.entry(ns_key).or_insert_with(HashMap::new);
        namespace_map.insert(key.to_string(), item);
    }

    async fn search(
        &self,
        namespace: &[&str],
        query: Option<&str>,
        filter: Option<&StoreFilter>,
        limit: usize,
    ) -> Result<Vec<StoreValue>, StoreError> {
        let ns_key = Self::make_namespace_key(namespace);
        let data = self.data.read().await;

        let namespace_map = data.get(&ns_key);
        if namespace_map.is_none() {
            return Ok(Vec::new());
        }
        let namespace_map = namespace_map.unwrap();

        // Collect all items
        let mut items: Vec<(&String, &StoredItem)> = namespace_map.iter().collect();

        // Apply filter if provided
        if let Some(filter) = filter {
            items.retain(|(_, item)| filter.matches(&item.value));
        }

        // If query is provided and vector index is enabled, perform similarity search
        if let Some(query_text) = query {
            if self.config.enable_vector_index {
                if let Some(embedder) = &self.config.embedder {
                    let query_vector_f64 = embedder
                        .embed_query(query_text)
                        .await
                        .map_err(|e| StoreError::EmbeddingError(e.to_string()))?;
                    // Convert f64 to f32
                    let query_vector: Vec<f32> =
                        query_vector_f64.into_iter().map(|x| x as f32).collect();

                    // Calculate similarities and sort
                    let mut scored_items: Vec<(f64, StoreValue)> = items
                        .iter()
                        .filter_map(|(_, item)| {
                            item.vector.as_ref().map(|vec| {
                                let similarity = Self::cosine_similarity(&query_vector, vec);
                                (similarity, item.value.clone())
                            })
                        })
                        .collect();

                    // Sort by similarity (descending)
                    scored_items
                        .sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

                    // Take top results
                    let results: Vec<StoreValue> = scored_items
                        .into_iter()
                        .take(limit)
                        .map(|(_, value)| value)
                        .collect();

                    return Ok(results);
                }
            }
        }

        // No vector search, just return filtered results
        let results: Vec<StoreValue> = items
            .iter()
            .take(limit)
            .map(|(_, item)| item.value.clone())
            .collect();

        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_enhanced_store_basic() {
        let store = EnhancedInMemoryStore::new();

        let store_value = StoreValue::new(serde_json::json!({"name": "John"}));
        store
            .put_with_metadata(&["users"], "user1", store_value)
            .await;

        let retrieved = store.get_with_metadata(&["users"], "user1").await;
        assert!(retrieved.is_some());
        assert_eq!(
            retrieved.unwrap().value,
            serde_json::json!({"name": "John"})
        );
    }

    #[tokio::test]
    async fn test_enhanced_store_search_by_filter() {
        let store = EnhancedInMemoryStore::new();

        store
            .put_with_metadata(
                &["users"],
                "user1",
                StoreValue::new(serde_json::json!({"name": "John", "age": 30})),
            )
            .await;

        store
            .put_with_metadata(
                &["users"],
                "user2",
                StoreValue::new(serde_json::json!({"name": "Jane", "age": 25})),
            )
            .await;

        let filter = StoreFilter::content_equals("age".to_string(), serde_json::json!(30));
        let results = store
            .search_by_filter(&["users"], &filter, 10)
            .await
            .unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].value["name"], "John");
    }

    #[tokio::test]
    async fn test_enhanced_store_metadata() {
        let store = EnhancedInMemoryStore::new();

        let mut metadata = std::collections::HashMap::new();
        metadata.insert("created_at".to_string(), serde_json::json!("2024-01-01"));

        let store_value =
            StoreValue::with_metadata(serde_json::json!({"name": "Test"}), metadata.clone());

        store
            .put_with_metadata(&["test"], "key1", store_value)
            .await;

        let retrieved = store.get_with_metadata(&["test"], "key1").await.unwrap();
        assert_eq!(retrieved.metadata, Some(metadata));
    }

    #[tokio::test]
    async fn test_enhanced_store_list() {
        let store = EnhancedInMemoryStore::new();

        store
            .put_with_metadata(
                &["test"],
                "key1",
                StoreValue::new(serde_json::json!("value1")),
            )
            .await;

        store
            .put_with_metadata(
                &["test"],
                "key2",
                StoreValue::new(serde_json::json!("value2")),
            )
            .await;

        let keys = store.list(&["test"]).await;
        assert_eq!(keys.len(), 2);
        assert!(keys.contains(&"key1".to_string()));
        assert!(keys.contains(&"key2".to_string()));
    }
}
