use schemars::{schema_for, JsonSchema};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::marker::PhantomData;

/// Trait for types that can be used as structured output schemas.
///
/// Types implementing this trait can be automatically converted to JSON schemas
/// and used for structured output generation.
pub trait StructuredOutputSchema:
    Serialize + for<'de> Deserialize<'de> + JsonSchema + Send + Sync
{
    /// Get the JSON schema for this type.
    fn schema() -> Value {
        let schema = schema_for!(Self);
        serde_json::to_value(&schema).unwrap_or_else(|_| {
            serde_json::json!({
                "type": "object",
                "description": "Generated schema"
            })
        })
    }

    /// Get the schema name (typically the type name).
    fn schema_name() -> String {
        std::any::type_name::<Self>()
            .split("::")
            .last()
            .unwrap_or("Unknown")
            .to_string()
    }

    /// Get a description for the schema (can be overridden).
    fn schema_description() -> Option<String> {
        None
    }
}

/// Strategy for handling structured output errors.
#[derive(Clone, Debug)]
pub enum ErrorHandlingStrategy {
    /// Handle all errors with default messages
    All,
    /// Handle only specific error types
    Specific(Vec<String>),
    /// Custom error handler function
    Custom(String), // Store function name/identifier
    /// No error handling, let errors propagate
    None,
}

impl Default for ErrorHandlingStrategy {
    fn default() -> Self {
        ErrorHandlingStrategy::All
    }
}

/// Provider-native structured output strategy.
///
/// Uses the model provider's native structured output capabilities
/// (e.g., OpenAI's response_format, Claude's structured output).
pub struct ProviderStrategy<T> {
    pub(crate) _phantom: PhantomData<T>,
    pub strict: Option<bool>,
}

impl<T> ProviderStrategy<T>
where
    T: StructuredOutputSchema,
{
    pub fn new() -> Self {
        Self {
            _phantom: PhantomData,
            strict: None,
        }
    }

    pub fn with_strict(mut self, strict: bool) -> Self {
        self.strict = Some(strict);
        self
    }

    pub fn schema(&self) -> Value {
        T::schema()
    }

    pub fn schema_name(&self) -> String {
        <T as StructuredOutputSchema>::schema_name()
    }
}

impl<T> Default for ProviderStrategy<T>
where
    T: StructuredOutputSchema,
{
    fn default() -> Self {
        Self::new()
    }
}

/// Tool-calling-based structured output strategy.
///
/// Uses tool calling to achieve structured output for models that don't
/// support native structured output.
pub struct ToolStrategy<T> {
    pub(crate) _phantom: PhantomData<T>,
    pub tool_message_content: Option<String>,
    pub handle_errors: ErrorHandlingStrategy,
}

impl<T> ToolStrategy<T>
where
    T: StructuredOutputSchema,
{
    pub fn new() -> Self {
        Self {
            _phantom: PhantomData,
            tool_message_content: None,
            handle_errors: ErrorHandlingStrategy::default(),
        }
    }

    pub fn with_tool_message_content(mut self, content: String) -> Self {
        self.tool_message_content = Some(content);
        self
    }

    pub fn with_error_handling(mut self, strategy: ErrorHandlingStrategy) -> Self {
        self.handle_errors = strategy;
        self
    }

    pub fn schema(&self) -> Value {
        T::schema()
    }

    pub fn schema_name(&self) -> String {
        <T as StructuredOutputSchema>::schema_name()
    }
}

impl<T> Default for ToolStrategy<T>
where
    T: StructuredOutputSchema,
{
    fn default() -> Self {
        Self::new()
    }
}

/// Auto-select strategy based on model capabilities.
pub struct AutoStrategy<T> {
    pub(crate) _phantom: PhantomData<T>,
}

impl<T> AutoStrategy<T>
where
    T: StructuredOutputSchema,
{
    pub fn new() -> Self {
        Self {
            _phantom: PhantomData,
        }
    }

    pub fn schema(&self) -> Value {
        T::schema()
    }

    pub fn schema_name(&self) -> String {
        <T as StructuredOutputSchema>::schema_name()
    }
}

impl<T> Default for AutoStrategy<T>
where
    T: StructuredOutputSchema,
{
    fn default() -> Self {
        Self::new()
    }
}

/// Trait for structured output strategies.
pub trait StructuredOutputStrategy: Send + Sync {
    fn schema(&self) -> Value;
    fn schema_name(&self) -> String;
    fn strategy_type(&self) -> StrategyType;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StrategyType {
    Provider,
    Tool,
    Auto,
}

impl<T> StructuredOutputStrategy for ProviderStrategy<T>
where
    T: StructuredOutputSchema,
{
    fn schema(&self) -> Value {
        ProviderStrategy::schema(self)
    }

    fn schema_name(&self) -> String {
        ProviderStrategy::schema_name(self)
    }

    fn strategy_type(&self) -> StrategyType {
        StrategyType::Provider
    }
}

impl<T> StructuredOutputStrategy for ToolStrategy<T>
where
    T: StructuredOutputSchema,
{
    fn schema(&self) -> Value {
        ToolStrategy::schema(self)
    }

    fn schema_name(&self) -> String {
        ToolStrategy::schema_name(self)
    }

    fn strategy_type(&self) -> StrategyType {
        StrategyType::Tool
    }
}

impl<T> StructuredOutputStrategy for AutoStrategy<T>
where
    T: StructuredOutputSchema,
{
    fn schema(&self) -> Value {
        AutoStrategy::schema(self)
    }

    fn schema_name(&self) -> String {
        AutoStrategy::schema_name(self)
    }

    fn strategy_type(&self) -> StrategyType {
        StrategyType::Auto
    }
}

/// Errors related to structured output.
#[derive(Debug, thiserror::Error)]
pub enum StructuredOutputError {
    #[error("Validation error: {0}")]
    ValidationError(#[from] serde_json::Error),

    #[error("Multiple structured outputs returned: {0:?}")]
    MultipleOutputs(Vec<String>),

    #[error("Parse error: {0}")]
    ParseError(String),

    #[error("Provider error: {0}")]
    ProviderError(String),

    #[error("Schema generation error: {0}")]
    SchemaError(String),
}

/// Validate a value against a JSON schema.
pub fn validate_against_schema(value: &Value, schema: &Value) -> Result<(), StructuredOutputError> {
    // Basic validation - check if value matches schema structure
    // For more strict validation, use jsonschema crate
    if let (Some(value_obj), Some(schema_obj)) = (value.as_object(), schema.as_object()) {
        if let Some(_properties) = schema_obj.get("properties").and_then(|v| v.as_object()) {
            if let Some(required) = schema_obj.get("required").and_then(|v| v.as_array()) {
                for req_field in required {
                    if let Some(field_name) = req_field.as_str() {
                        if !value_obj.contains_key(field_name) {
                            return Err(StructuredOutputError::ParseError(format!(
                                "Missing required field: {}",
                                field_name
                            )));
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use schemars::JsonSchema;

    #[derive(Serialize, Deserialize, JsonSchema, Debug)]
    struct TestSchema {
        name: String,
        age: i32,
    }

    impl StructuredOutputSchema for TestSchema {}

    #[test]
    fn test_provider_strategy() {
        let strategy = ProviderStrategy::<TestSchema>::new();
        let schema = strategy.schema();
        assert!(schema.is_object());
        assert_eq!(strategy.schema_name(), "TestSchema");
    }

    #[test]
    fn test_tool_strategy() {
        let strategy =
            ToolStrategy::<TestSchema>::new().with_tool_message_content("Test message".to_string());
        let schema = strategy.schema();
        assert!(schema.is_object());
        assert_eq!(
            strategy.tool_message_content,
            Some("Test message".to_string())
        );
    }

    #[test]
    fn test_auto_strategy() {
        let strategy = AutoStrategy::<TestSchema>::new();
        let schema = strategy.schema();
        assert!(schema.is_object());
    }

    #[test]
    fn test_schema_generation() {
        let schema = TestSchema::schema();
        assert!(schema.is_object());
        let schema_obj = schema.as_object().unwrap();
        assert!(schema_obj.contains_key("$schema") || schema_obj.contains_key("type"));
    }
}
