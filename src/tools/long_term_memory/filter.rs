use serde_json::Value;

/// Filter for querying stored values in long-term memory.
///
/// Supports filtering by content values, metadata, and combining multiple filters.
#[derive(Clone, Debug)]
pub enum StoreFilter {
    /// Filter by content value equality
    ContentEquals {
        /// JSON path to the field (e.g., "name" or "user.preferences.language")
        key: String,
        /// Value to match
        value: Value,
    },
    /// Filter by content value containing a string
    ContentContains {
        /// JSON path to the field
        key: String,
        /// String to search for
        value: String,
    },
    /// Filter by metadata value equality
    MetadataEquals {
        /// Metadata key
        key: String,
        /// Value to match
        value: Value,
    },
    /// Filter by metadata containing a string
    MetadataContains {
        /// Metadata key
        key: String,
        /// String to search for
        value: String,
    },
    /// Logical AND - all filters must match
    And(Vec<StoreFilter>),
    /// Logical OR - any filter must match
    Or(Vec<StoreFilter>),
}

impl StoreFilter {
    /// Create a content equality filter
    pub fn content_equals(key: String, value: Value) -> Self {
        Self::ContentEquals { key, value }
    }

    /// Create a content contains filter
    pub fn content_contains(key: String, value: String) -> Self {
        Self::ContentContains { key, value }
    }

    /// Create a metadata equality filter
    pub fn metadata_equals(key: String, value: Value) -> Self {
        Self::MetadataEquals { key, value }
    }

    /// Create a metadata contains filter
    pub fn metadata_contains(key: String, value: String) -> Self {
        Self::MetadataContains { key, value }
    }

    /// Create an AND filter
    pub fn and(filters: Vec<StoreFilter>) -> Self {
        Self::And(filters)
    }

    /// Create an OR filter
    pub fn or(filters: Vec<StoreFilter>) -> Self {
        Self::Or(filters)
    }

    /// Check if a StoreValue matches this filter
    pub fn matches(&self, store_value: &crate::tools::long_term_memory::StoreValue) -> bool {
        match self {
            Self::ContentEquals { key, value } => Self::get_value_by_path(&store_value.value, key)
                .map(|v| v == value)
                .unwrap_or(false),
            Self::ContentContains { key, value } => {
                Self::get_value_by_path(&store_value.value, key)
                    .and_then(|v| v.as_str())
                    .map(|s| s.contains(value))
                    .unwrap_or(false)
            }
            Self::MetadataEquals { key, value } => store_value
                .metadata
                .as_ref()
                .and_then(|m| m.get(key))
                .map(|v| v == value)
                .unwrap_or(false),
            Self::MetadataContains { key, value } => store_value
                .metadata
                .as_ref()
                .and_then(|m| m.get(key))
                .and_then(|v| v.as_str())
                .map(|s| s.contains(value))
                .unwrap_or(false),
            Self::And(filters) => filters.iter().all(|f| f.matches(store_value)),
            Self::Or(filters) => filters.iter().any(|f| f.matches(store_value)),
        }
    }

    /// Get a value from a JSON object by path (supports simple dot notation)
    fn get_value_by_path<'a>(obj: &'a Value, path: &str) -> Option<&'a Value> {
        let parts: Vec<&str> = path.split('.').collect();
        let mut current = obj;

        for part in parts {
            match current {
                Value::Object(map) => {
                    current = map.get(part)?;
                }
                _ => return None,
            }
        }

        Some(current)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::long_term_memory::StoreValue;

    #[test]
    fn test_content_equals_filter() {
        let store_value = StoreValue::new(serde_json::json!({"name": "John", "age": 30}));

        let filter = StoreFilter::content_equals("name".to_string(), serde_json::json!("John"));
        assert!(filter.matches(&store_value));

        let filter = StoreFilter::content_equals("name".to_string(), serde_json::json!("Jane"));
        assert!(!filter.matches(&store_value));
    }

    #[test]
    fn test_metadata_equals_filter() {
        let mut metadata = std::collections::HashMap::new();
        metadata.insert("language".to_string(), serde_json::json!("English"));

        let store_value = StoreValue::with_metadata(serde_json::json!({"name": "John"}), metadata);

        let filter =
            StoreFilter::metadata_equals("language".to_string(), serde_json::json!("English"));
        assert!(filter.matches(&store_value));
    }

    #[test]
    fn test_and_filter() {
        let store_value = StoreValue::new(serde_json::json!({"name": "John", "age": 30}));

        let filter = StoreFilter::and(vec![
            StoreFilter::content_equals("name".to_string(), serde_json::json!("John")),
            StoreFilter::content_equals("age".to_string(), serde_json::json!(30)),
        ]);
        assert!(filter.matches(&store_value));

        let filter = StoreFilter::and(vec![
            StoreFilter::content_equals("name".to_string(), serde_json::json!("John")),
            StoreFilter::content_equals("age".to_string(), serde_json::json!(25)),
        ]);
        assert!(!filter.matches(&store_value));
    }

    #[test]
    fn test_or_filter() {
        let store_value = StoreValue::new(serde_json::json!({"name": "John", "age": 30}));

        let filter = StoreFilter::or(vec![
            StoreFilter::content_equals("name".to_string(), serde_json::json!("Jane")),
            StoreFilter::content_equals("age".to_string(), serde_json::json!(30)),
        ]);
        assert!(filter.matches(&store_value));

        let filter = StoreFilter::or(vec![
            StoreFilter::content_equals("name".to_string(), serde_json::json!("Jane")),
            StoreFilter::content_equals("age".to_string(), serde_json::json!(25)),
        ]);
        assert!(!filter.matches(&store_value));
    }

    #[test]
    fn test_content_contains_filter() {
        let store_value = StoreValue::new(serde_json::json!({"description": "A test description"}));

        let filter = StoreFilter::content_contains("description".to_string(), "test".to_string());
        assert!(filter.matches(&store_value));

        let filter =
            StoreFilter::content_contains("description".to_string(), "missing".to_string());
        assert!(!filter.matches(&store_value));
    }
}
