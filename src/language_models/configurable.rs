use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashMap;

use crate::{
    language_models::{
        invocation_config::InvocationConfig,
        llm::{LLMClone, LLM},
        options::CallOptions,
        GenerateResult, LLMError,
    },
    schemas::Message,
};
use futures::Stream;
use std::pin::Pin;

/// A wrapper around an LLM that allows runtime configuration of certain fields.
///
/// This enables you to switch models or modify parameters at runtime without
/// creating a new model instance, similar to LangChain Python's configurable models.
///
/// # Example
/// ```rust,ignore
/// use oris::language_models::{init_chat_model, ConfigurableModel};
///
/// let base_model = init_chat_model("gpt-4o-mini", None, None, None, None, None, None, None)?;
/// let configurable = ConfigurableModel::new(base_model)
///     .with_configurable_fields(vec!["model".to_string(), "temperature".to_string()])
///     .with_config_prefix("first".to_string());
///
/// // Later, use with different config
/// let config = InvocationConfig::new()
///     .add_metadata("first_model".to_string(), json!("claude-sonnet-4-5-20250929"));
/// ```
pub struct ConfigurableModel {
    model: Box<dyn LLM>,
    configurable_fields: Vec<String>,
    config_prefix: Option<String>,
    default_config: HashMap<String, Value>,
}

impl ConfigurableModel {
    pub fn new(model: Box<dyn LLM>) -> Self {
        Self {
            model,
            configurable_fields: vec!["model".to_string(), "model_provider".to_string()],
            config_prefix: None,
            default_config: HashMap::new(),
        }
    }

    pub fn with_configurable_fields(mut self, fields: Vec<String>) -> Self {
        self.configurable_fields = fields;
        self
    }

    pub fn with_config_prefix(mut self, prefix: String) -> Self {
        self.config_prefix = Some(prefix);
        self
    }

    pub fn with_default_config(mut self, key: String, value: Value) -> Self {
        self.default_config.insert(key, value);
        self
    }

    fn get_config_value<'a>(
        &'a self,
        config: &'a InvocationConfig,
        field: &str,
    ) -> Option<&'a Value> {
        let key: String = if let Some(ref prefix) = self.config_prefix {
            format!("{}_{}", prefix, field)
        } else {
            field.to_string()
        };
        config.metadata.get(&key)
    }

    fn should_override(&self, field: &str) -> bool {
        self.configurable_fields.contains(&field.to_string())
    }
}

#[async_trait]
impl LLM for ConfigurableModel {
    async fn generate(&self, messages: &[Message]) -> Result<GenerateResult, LLMError> {
        // For now, just delegate to the underlying model
        // In a full implementation, we would check config and potentially
        // create a new model instance with overridden parameters
        self.model.generate(messages).await
    }

    async fn invoke(&self, prompt: &str) -> Result<String, LLMError> {
        self.model.invoke(prompt).await
    }

    async fn stream(
        &self,
        messages: &[Message],
    ) -> Result<
        Pin<Box<dyn Stream<Item = Result<crate::schemas::StreamData, LLMError>> + Send>>,
        LLMError,
    > {
        self.model.stream(messages).await
    }

    fn add_options(&mut self, options: CallOptions) {
        self.model.add_options(options);
    }

    fn messages_to_string(&self, messages: &[Message]) -> String {
        self.model.messages_to_string(messages)
    }
}

impl LLMClone for ConfigurableModel {
    fn clone_box(&self) -> Box<dyn LLM> {
        // Note: This is a limitation - we can't clone Box<dyn LLM>
        // In practice, configurable models should wrap cloneable models
        panic!("ConfigurableModel cannot be cloned. Use a cloneable model type instead.")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::language_models::init_chat_model;

    #[tokio::test]
    async fn test_configurable_model_creation() {
        let base_model = init_chat_model("gpt-4o-mini", None, None, None, None, None, None, None)
            .await
            .unwrap();
        let configurable = ConfigurableModel::new(base_model)
            .with_configurable_fields(vec!["model".to_string(), "temperature".to_string()]);

        assert_eq!(configurable.configurable_fields.len(), 2);
    }

    #[tokio::test]
    async fn test_configurable_model_prefix() {
        let base_model = init_chat_model("gpt-4o-mini", None, None, None, None, None, None, None)
            .await
            .unwrap();
        let configurable =
            ConfigurableModel::new(base_model).with_config_prefix("first".to_string());

        assert_eq!(configurable.config_prefix, Some("first".to_string()));
    }
}
