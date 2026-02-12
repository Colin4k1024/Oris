use async_trait::async_trait;
use serde_json::Value;

/// Persistent store for long-term memory across conversations.
///
/// The store allows tools to save and retrieve data that persists
/// beyond a single conversation session.
#[async_trait]
pub trait ToolStore: Send + Sync {
    /// Get a value from the store by namespace and key
    async fn get(&self, namespace: &[&str], key: &str) -> Option<Value>;

    /// Put a value into the store
    async fn put(&self, namespace: &[&str], key: &str, value: Value);

    /// Delete a value from the store
    async fn delete(&self, namespace: &[&str], key: &str);

    /// List all keys in a namespace
    async fn list(&self, _namespace: &[&str]) -> Vec<String> {
        vec![] // Default implementation returns empty
    }
}

/// In-memory store implementation for testing and simple use cases.
#[derive(Clone, Debug)]
pub struct InMemoryStore {
    data: std::sync::Arc<tokio::sync::RwLock<std::collections::HashMap<String, Value>>>,
}

impl InMemoryStore {
    pub fn new() -> Self {
        Self {
            data: std::sync::Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
        }
    }

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
impl ToolStore for InMemoryStore {
    async fn get(&self, namespace: &[&str], key: &str) -> Option<Value> {
        let store_key = Self::make_key(namespace, key);
        let data = self.data.read().await;
        data.get(&store_key).cloned()
    }

    async fn put(&self, namespace: &[&str], key: &str, value: Value) {
        let store_key = Self::make_key(namespace, key);
        let mut data = self.data.write().await;
        data.insert(store_key, value);
    }

    async fn delete(&self, namespace: &[&str], key: &str) {
        let store_key = Self::make_key(namespace, key);
        let mut data = self.data.write().await;
        data.remove(&store_key);
    }

    async fn list(&self, namespace: &[&str]) -> Vec<String> {
        let prefix = if namespace.is_empty() {
            String::new()
        } else {
            format!("{}:", namespace.join(":"))
        };

        let data = self.data.read().await;
        data.keys()
            .filter_map(|k| {
                if prefix.is_empty() {
                    Some(k.clone())
                } else if k.starts_with(&prefix) {
                    k.strip_prefix(&prefix).map(|s| s.to_string())
                } else {
                    None
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_in_memory_store() {
        let store = InMemoryStore::new();

        // Test put and get
        store
            .put(&["users"], "user1", serde_json::json!({"name": "Alice"}))
            .await;
        let value = store.get(&["users"], "user1").await;
        assert_eq!(value, Some(serde_json::json!({"name": "Alice"})));

        // Test delete
        store.delete(&["users"], "user1").await;
        let value = store.get(&["users"], "user1").await;
        assert_eq!(value, None);
    }

    #[tokio::test]
    async fn test_store_namespaces() {
        let store = InMemoryStore::new();

        store
            .put(&["users"], "user1", serde_json::json!("value1"))
            .await;
        store
            .put(&["preferences"], "pref1", serde_json::json!("value2"))
            .await;

        let user_value = store.get(&["users"], "user1").await;
        let pref_value = store.get(&["preferences"], "pref1").await;

        assert_eq!(user_value, Some(serde_json::json!("value1")));
        assert_eq!(pref_value, Some(serde_json::json!("value2")));
    }

    #[tokio::test]
    async fn test_store_list() {
        let store = InMemoryStore::new();

        store
            .put(&["users"], "user1", serde_json::json!("value1"))
            .await;
        store
            .put(&["users"], "user2", serde_json::json!("value2"))
            .await;
        store
            .put(&["preferences"], "pref1", serde_json::json!("value3"))
            .await;

        let users = store.list(&["users"]).await;
        assert!(users.contains(&"user1".to_string()));
        assert!(users.contains(&"user2".to_string()));
        assert!(!users.contains(&"pref1".to_string()));
    }
}
