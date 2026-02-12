use crate::language_models::TokenUsage;
use std::collections::HashMap;

/// Aggregated usage metadata across multiple model invocations.
///
/// This tracks token usage per model, allowing you to monitor
/// resource consumption across different models in your application.
///
/// # Example
/// ```rust,ignore
/// let mut usage = UsageMetadata::new();
/// usage.add_usage("gpt-4o-mini", &token_usage);
/// usage.add_usage("gpt-4o-mini", &token_usage2);
/// let total = usage.get_total_usage("gpt-4o-mini");
/// ```
#[derive(Debug, Clone, Default)]
pub struct UsageMetadata {
    usage_by_model: HashMap<String, TokenUsage>,
}

impl UsageMetadata {
    pub fn new() -> Self {
        Self {
            usage_by_model: HashMap::new(),
        }
    }

    /// Add token usage for a specific model.
    pub fn add_usage(&mut self, model: &str, usage: &TokenUsage) {
        let entry = self
            .usage_by_model
            .entry(model.to_string())
            .or_insert_with(|| TokenUsage::new(0, 0));
        entry.add(usage);
    }

    /// Get total usage for a specific model.
    pub fn get_total_usage(&self, model: &str) -> Option<&TokenUsage> {
        self.usage_by_model.get(model)
    }

    /// Get all usage data.
    pub fn get_all_usage(&self) -> &HashMap<String, TokenUsage> {
        &self.usage_by_model
    }

    /// Clear all usage data.
    pub fn clear(&mut self) {
        self.usage_by_model.clear();
    }

    /// Merge another UsageMetadata into this one.
    pub fn merge(&mut self, other: UsageMetadata) {
        for (model, usage) in other.usage_by_model {
            self.add_usage(&model, &usage);
        }
    }
}

/// Trait for callbacks that track token usage.
///
/// This allows you to implement custom usage tracking logic,
/// such as logging to a database or sending metrics to a monitoring system.
pub trait UsageCallback: Send + Sync {
    fn on_usage(&mut self, model: &str, usage: &TokenUsage);
}

/// Default implementation that collects usage in memory.
#[derive(Debug, Default)]
pub struct CollectingUsageCallback {
    metadata: UsageMetadata,
}

impl CollectingUsageCallback {
    pub fn new() -> Self {
        Self {
            metadata: UsageMetadata::new(),
        }
    }

    pub fn get_metadata(&self) -> &UsageMetadata {
        &self.metadata
    }

    pub fn into_metadata(self) -> UsageMetadata {
        self.metadata
    }
}

impl UsageCallback for CollectingUsageCallback {
    fn on_usage(&mut self, model: &str, usage: &TokenUsage) {
        self.metadata.add_usage(model, usage);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_usage_metadata_add() {
        let mut usage = UsageMetadata::new();
        let token_usage = TokenUsage::new(10, 20);
        usage.add_usage("gpt-4o-mini", &token_usage);

        let total = usage.get_total_usage("gpt-4o-mini").unwrap();
        assert_eq!(total.prompt_tokens, 10);
        assert_eq!(total.completion_tokens, 20);
        assert_eq!(total.total_tokens, 30);
    }

    #[test]
    fn test_usage_metadata_accumulate() {
        let mut usage = UsageMetadata::new();
        usage.add_usage("gpt-4o-mini", &TokenUsage::new(10, 20));
        usage.add_usage("gpt-4o-mini", &TokenUsage::new(5, 10));

        let total = usage.get_total_usage("gpt-4o-mini").unwrap();
        assert_eq!(total.prompt_tokens, 15);
        assert_eq!(total.completion_tokens, 30);
        assert_eq!(total.total_tokens, 45);
    }

    #[test]
    fn test_usage_metadata_merge() {
        let mut usage1 = UsageMetadata::new();
        usage1.add_usage("gpt-4o-mini", &TokenUsage::new(10, 20));

        let mut usage2 = UsageMetadata::new();
        usage2.add_usage("gpt-4o-mini", &TokenUsage::new(5, 10));
        usage2.add_usage("claude-3", &TokenUsage::new(15, 25));

        usage1.merge(usage2);

        let gpt_usage = usage1.get_total_usage("gpt-4o-mini").unwrap();
        assert_eq!(gpt_usage.total_tokens, 45);

        let claude_usage = usage1.get_total_usage("claude-3").unwrap();
        assert_eq!(claude_usage.total_tokens, 40);
    }

    #[test]
    fn test_collecting_callback() {
        let mut callback = CollectingUsageCallback::new();
        callback.on_usage("gpt-4o-mini", &TokenUsage::new(10, 20));

        let metadata = callback.get_metadata();
        let usage = metadata.get_total_usage("gpt-4o-mini").unwrap();
        assert_eq!(usage.total_tokens, 30);
    }
}
