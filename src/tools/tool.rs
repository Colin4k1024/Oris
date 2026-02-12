use std::error::Error;
use std::string::String;

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::agent::Command;
use crate::error::ToolError;

use super::runtime::ToolRuntime;

#[async_trait]
pub trait Tool: Send + Sync {
    /// Returns the name of the tool.
    fn name(&self) -> String;

    /// Provides a description of what the tool does and when to use it.
    fn description(&self) -> String;
    /// This are the parametters for OpenAi-like function call.
    /// You should return a jsnon like this one
    /// ```json
    /// {
    ///     "type": "object",
    ///     "properties": {
    ///         "command": {
    ///             "type": "string",
    ///             "description": "The raw command you want executed"
    ///                 }
    ///     },
    ///     "required": ["command"]
    /// }
    ///
    /// If there s no implementation the defaul will be the self.description()
    ///```
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
                "properties": {
                "input": {
                    "type": "string",
                    "description":self.description()
                }
            },
            "required": ["input"]
        })
    }

    /// Processes an input string and executes the tool's functionality, returning a `Result`.
    ///
    /// This function utilizes `parse_input` to parse the input and then calls `run`.
    /// Its used by the Agent
    async fn call(&self, input: &str) -> Result<String, ToolError> {
        let input = self.parse_input(input).await;
        self.run(input).await
    }

    /// Executes the core functionality of the tool.
    ///
    /// Example implementation:
    /// ```rust,ignore
    /// async fn run(&self, input: Value) -> Result<String, Box<dyn Error>> {
    ///     let input_str = input.as_str().ok_or("Input should be a string")?;
    ///     self.simple_search(input_str).await
    /// }
    /// ```
    async fn run(&self, input: Value) -> Result<String, ToolError>;

    /// Executes the tool with runtime access (state, context, store, etc.).
    ///
    /// This method is called when the tool needs access to runtime information.
    /// The default implementation calls `run()` for backward compatibility.
    ///
    /// Tools that need runtime access should override this method.
    /// Tools can return a `Command` to update agent state.
    ///
    /// Example implementation:
    /// ```rust,ignore
    /// async fn run_with_runtime(
    ///     &self,
    ///     input: Value,
    ///     runtime: &ToolRuntime,
    /// ) -> Result<ToolResult, Box<dyn Error>> {
    ///     let state = runtime.state().await;
    ///     let messages = &state.messages;
    ///     // Use runtime information
    ///     Ok(ToolResult::Text("result".to_string()))
    /// }
    /// ```
    async fn run_with_runtime(
        &self,
        input: Value,
        _runtime: &ToolRuntime,
    ) -> Result<ToolResult, Box<dyn Error>> {
        // Default implementation calls run() for backward compatibility
        let result = self.run(input).await?;
        Ok(ToolResult::Text(result))
    }

    /// Check if this tool requires runtime access.
    ///
    /// Returns `true` if the tool needs access to runtime information
    /// (state, context, store, etc.). Default is `false`.
    fn requires_runtime(&self) -> bool {
        false
    }

    /// Parses the input string, which could be a JSON value or a raw string, depending on the LLM model.
    ///
    /// Implement this function to extract the parameters needed for your tool. If a simple
    /// string is sufficient, the default implementation can be used.
    async fn parse_input(&self, input: &str) -> Value {
        log::info!("Using default implementation: {}", input);
        match serde_json::from_str::<Value>(input) {
            Ok(input) => {
                if input["input"].is_string() {
                    Value::String(input["input"].as_str().unwrap().to_string())
                } else {
                    Value::String(input.to_string())
                }
            }
            Err(_) => Value::String(input.to_string()),
        }
    }
}

/// Result type for tool execution that can return either text or a command.
#[derive(Debug)]
pub enum ToolResult {
    /// Simple text result (backward compatible)
    Text(String),
    /// Result with a command to update state
    WithCommand {
        text: String,
        command: Option<Command>,
    },
}

impl ToolResult {
    pub fn text(text: String) -> Self {
        Self::Text(text)
    }

    pub fn with_command(text: String, command: Command) -> Self {
        Self::WithCommand {
            text,
            command: Some(command),
        }
    }

    pub fn into_string(self) -> String {
        match self {
            Self::Text(s) => s,
            Self::WithCommand { text, .. } => text,
        }
    }
}

impl From<String> for ToolResult {
    fn from(s: String) -> Self {
        Self::Text(s)
    }
}
