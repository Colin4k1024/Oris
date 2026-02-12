use serde_json::Value;
use std::collections::HashMap;

/// Configuration for model invocations at runtime.
///
/// This allows you to pass additional configuration when invoking a model,
/// such as run names, tags, metadata, and other execution control parameters.
///
/// # Example
/// ```rust,ignore
/// let config = InvocationConfig::new()
///     .with_run_name("my_run".to_string())
///     .with_tags(vec!["production".to_string(), "experiment".to_string()])
///     .with_metadata({
///         let mut m = HashMap::new();
///         m.insert("user_id".to_string(), json!("123"));
///         m
///     });
/// ```
#[derive(Debug, Clone, Default)]
pub struct InvocationConfig {
    pub run_name: Option<String>,
    pub tags: Vec<String>,
    pub metadata: HashMap<String, Value>,
    pub max_concurrency: Option<usize>,
    pub recursion_limit: Option<usize>,
}

impl InvocationConfig {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_run_name(mut self, name: String) -> Self {
        self.run_name = Some(name);
        self
    }

    pub fn with_tags(mut self, tags: Vec<String>) -> Self {
        self.tags = tags;
        self
    }

    pub fn add_tag(mut self, tag: String) -> Self {
        self.tags.push(tag);
        self
    }

    pub fn with_metadata(mut self, metadata: HashMap<String, Value>) -> Self {
        self.metadata = metadata;
        self
    }

    pub fn add_metadata(mut self, key: String, value: Value) -> Self {
        self.metadata.insert(key, value);
        self
    }

    pub fn with_max_concurrency(mut self, max_concurrency: usize) -> Self {
        self.max_concurrency = Some(max_concurrency);
        self
    }

    pub fn with_recursion_limit(mut self, recursion_limit: usize) -> Self {
        self.recursion_limit = Some(recursion_limit);
        self
    }

    pub fn merge(&mut self, other: InvocationConfig) {
        // Run name is not inherited - each invocation has its own
        // Tags are merged
        self.tags.extend(other.tags);
        // Metadata is merged
        for (k, v) in other.metadata {
            self.metadata.insert(k, v);
        }
        // max_concurrency and recursion_limit prefer incoming if set
        if other.max_concurrency.is_some() {
            self.max_concurrency = other.max_concurrency;
        }
        if other.recursion_limit.is_some() {
            self.recursion_limit = other.recursion_limit;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_invocation_config_new() {
        let config = InvocationConfig::new();
        assert!(config.run_name.is_none());
        assert!(config.tags.is_empty());
        assert!(config.metadata.is_empty());
    }

    #[test]
    fn test_invocation_config_builder() {
        let config = InvocationConfig::new()
            .with_run_name("test_run".to_string())
            .with_tags(vec!["tag1".to_string(), "tag2".to_string()])
            .add_metadata("key1".to_string(), json!("value1"));

        assert_eq!(config.run_name, Some("test_run".to_string()));
        assert_eq!(config.tags.len(), 2);
        assert_eq!(config.metadata.get("key1"), Some(&json!("value1")));
    }

    #[test]
    fn test_invocation_config_merge() {
        let mut config1 = InvocationConfig::new()
            .with_tags(vec!["tag1".to_string()])
            .add_metadata("key1".to_string(), json!("value1"));

        let config2 = InvocationConfig::new()
            .with_tags(vec!["tag2".to_string()])
            .add_metadata("key2".to_string(), json!("value2"));

        config1.merge(config2);

        assert_eq!(config1.tags.len(), 2);
        assert!(config1.tags.contains(&"tag1".to_string()));
        assert!(config1.tags.contains(&"tag2".to_string()));
        assert_eq!(config1.metadata.len(), 2);
    }
}
