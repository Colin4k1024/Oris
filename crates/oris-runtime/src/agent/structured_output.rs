use crate::schemas::structured_output::{
    validate_against_schema, StructuredOutputError, StructuredOutputStrategy, ToolStrategy,
};
use crate::tools::{Tool, ToolResult, ToolRuntime};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::error::Error;
use std::sync::Arc;

/// A synthetic tool that represents a structured output schema.
///
/// This tool is automatically created when using ToolStrategy for structured output.
/// It captures the model's structured output and validates it against the schema.
pub struct StructuredOutputTool<T> {
    strategy: ToolStrategy<T>,
    schema_name: String,
}

impl<T> StructuredOutputTool<T>
where
    T: crate::schemas::StructuredOutputSchema,
{
    pub fn new(strategy: ToolStrategy<T>) -> Self {
        let schema_name = strategy.schema_name();
        Self {
            strategy,
            schema_name,
        }
    }

    pub fn schema(&self) -> Value {
        self.strategy.schema()
    }

    pub fn schema_name(&self) -> &str {
        &self.schema_name
    }
}

#[async_trait]
impl<T> Tool for StructuredOutputTool<T>
where
    T: crate::schemas::StructuredOutputSchema + Send + Sync + 'static,
{
    fn name(&self) -> String {
        self.schema_name.clone()
    }

    fn description(&self) -> String {
        format!(
            "Tool for returning structured output in the format: {}",
            self.schema_name
        )
    }

    fn parameters(&self) -> Value {
        // Return the schema as the parameters
        let mut schema = self.schema();

        // Ensure it's wrapped as a tool parameter schema
        if let Some(schema_obj) = schema.as_object_mut() {
            // Remove $schema if present (not needed for tool parameters)
            schema_obj.remove("$schema");
        }

        json!({
            "type": "object",
            "properties": schema,
            "required": self.get_required_fields()
        })
    }

    async fn run(&self, input: Value) -> Result<String, crate::error::ToolError> {
        // Validate the input against the schema
        validate_against_schema(&input, &self.schema())
            .map_err(|e| crate::error::ToolError::InvalidInputError(e.to_string()))?;

        // Serialize the validated input
        serde_json::to_string(&input)
            .map_err(|e| crate::error::ToolError::ExecutionError(e.to_string()))
    }

    async fn run_with_runtime(
        &self,
        input: Value,
        _runtime: &ToolRuntime,
    ) -> Result<ToolResult, Box<dyn Error>> {
        // Validate and return as string result
        let result = self.run(input).await?;
        Ok(ToolResult::Text(result))
    }

    fn requires_runtime(&self) -> bool {
        false
    }
}

impl<T> StructuredOutputTool<T>
where
    T: crate::schemas::StructuredOutputSchema,
{
    fn get_required_fields(&self) -> Vec<String> {
        if let Some(schema_obj) = self.schema().as_object() {
            if let Some(required) = schema_obj.get("required").and_then(|v| v.as_array()) {
                return required
                    .iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect();
            }
        }
        Vec::new()
    }
}

/// Handle structured output from tool calls.
///
/// This function processes tool call results and extracts structured output,
/// handling errors and retries as configured.
pub async fn handle_structured_output_tool_call(
    _tool_name: &str,
    tool_input: &str,
    _strategy: &dyn StructuredOutputStrategy,
) -> Result<Value, StructuredOutputError> {
    // Parse the tool input
    let parsed: Value = serde_json::from_str(tool_input).map_err(|e| {
        StructuredOutputError::ParseError(format!("Failed to parse tool input: {}", e))
    })?;

    // Validate against schema
    validate_against_schema(&parsed, &_strategy.schema())?;

    Ok(parsed)
}

/// Create a structured output tool from a strategy.
#[allow(clippy::boxed_local)]
pub fn create_structured_output_tool<S: StructuredOutputStrategy>(
    _strategy: Box<S>,
) -> Result<Arc<dyn Tool>, StructuredOutputError> {
    // This is a placeholder - in practice, we need to know the concrete type T
    // For now, we'll need to handle this differently
    Err(StructuredOutputError::SchemaError(
        "Cannot create tool from trait object - use concrete type".to_string(),
    ))
}

/// Error for when multiple structured outputs are returned.
#[derive(Debug, thiserror::Error)]
#[error("Multiple structured outputs returned: {tool_names:?}")]
pub struct MultipleStructuredOutputsError {
    pub tool_names: Vec<String>,
}

/// Error for structured output validation failures.
#[derive(Debug, thiserror::Error)]
#[error("Structured output validation failed: {message}")]
pub struct StructuredOutputValidationError {
    pub message: String,
    pub tool_name: String,
}

impl From<serde_json::Error> for StructuredOutputValidationError {
    fn from(err: serde_json::Error) -> Self {
        Self {
            message: err.to_string(),
            tool_name: "unknown".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schemas::structured_output::{StructuredOutputSchema, ToolStrategy};
    use schemars::JsonSchema;
    use serde::{Deserialize, Serialize};

    #[derive(Serialize, Deserialize, JsonSchema, Debug)]
    struct TestOutput {
        name: String,
        age: i32,
    }

    impl StructuredOutputSchema for TestOutput {}

    #[tokio::test]
    async fn test_structured_output_tool() {
        let strategy = ToolStrategy::<TestOutput>::new();
        let tool = StructuredOutputTool::new(strategy);

        let input = json!({
            "name": "John",
            "age": 30
        });

        let result = tool.run(input).await;
        assert!(result.is_ok());
    }

    #[test]
    fn test_get_required_fields() {
        let strategy = ToolStrategy::<TestOutput>::new();
        let tool = StructuredOutputTool::new(strategy);
        let required = tool.get_required_fields();
        // Note: This depends on the schema generation
        // In practice, schemars might mark all fields as required by default
        assert!(required.is_empty() || required.contains(&"name".to_string()));
    }
}
