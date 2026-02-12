use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A value stored in the long-term memory store, containing both the actual value and optional metadata.
///
/// This matches LangChain's StoreValue structure, allowing tools to store and retrieve
/// data with associated metadata for better organization and querying.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StoreValue {
    /// The actual value stored
    pub value: Value,
    /// Optional metadata associated with this value
    pub metadata: Option<std::collections::HashMap<String, Value>>,
}

impl StoreValue {
    /// Create a new StoreValue with just a value
    pub fn new(value: Value) -> Self {
        Self {
            value,
            metadata: None,
        }
    }

    /// Create a new StoreValue with value and metadata
    pub fn with_metadata(value: Value, metadata: std::collections::HashMap<String, Value>) -> Self {
        Self {
            value,
            metadata: Some(metadata),
        }
    }

    /// Get a reference to the value
    pub fn value(&self) -> &Value {
        &self.value
    }

    /// Get a reference to the metadata
    pub fn metadata(&self) -> Option<&std::collections::HashMap<String, Value>> {
        self.metadata.as_ref()
    }

    /// Get a mutable reference to the metadata
    pub fn metadata_mut(&mut self) -> &mut std::collections::HashMap<String, Value> {
        if self.metadata.is_none() {
            self.metadata = Some(std::collections::HashMap::new());
        }
        self.metadata.as_mut().unwrap()
    }

    /// Add a metadata entry
    pub fn add_metadata(&mut self, key: String, value: Value) {
        if self.metadata.is_none() {
            self.metadata = Some(std::collections::HashMap::new());
        }
        self.metadata.as_mut().unwrap().insert(key, value);
    }

    /// Get a metadata value by key
    pub fn get_metadata(&self, key: &str) -> Option<&Value> {
        self.metadata.as_ref()?.get(key)
    }
}

impl From<Value> for StoreValue {
    fn from(value: Value) -> Self {
        Self::new(value)
    }
}

impl From<StoreValue> for Value {
    fn from(store_value: StoreValue) -> Self {
        store_value.value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_store_value_creation() {
        let value = StoreValue::new(serde_json::json!({"name": "test"}));
        assert_eq!(value.value, serde_json::json!({"name": "test"}));
        assert!(value.metadata.is_none());
    }

    #[test]
    fn test_store_value_with_metadata() {
        let mut metadata = std::collections::HashMap::new();
        metadata.insert("created_at".to_string(), serde_json::json!("2024-01-01"));

        let value =
            StoreValue::with_metadata(serde_json::json!({"name": "test"}), metadata.clone());
        assert_eq!(value.metadata, Some(metadata));
    }

    #[test]
    fn test_store_value_metadata_operations() {
        let mut store_value = StoreValue::new(serde_json::json!({"name": "test"}));
        store_value.add_metadata("key1".to_string(), serde_json::json!("value1"));

        assert_eq!(
            store_value.get_metadata("key1"),
            Some(&serde_json::json!("value1"))
        );
    }
}
