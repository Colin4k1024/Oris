use std::error::Error;
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;

use crate::{
    agent::UnifiedAgent,
    schemas::messages::Message,
    tools::{Tool, ToolResult, ToolRuntime},
};

/// A tool that wraps a UnifiedAgent, allowing it to be used as a subagent.
///
/// This enables the Subagents pattern where a main agent coordinates
/// subagents as tools.
pub struct SubagentTool {
    /// The agent to be used as a subagent
    agent: Arc<UnifiedAgent>,
    /// Name of the subagent (used as tool name)
    name: String,
    /// Description of what this subagent does
    description: String,
}

impl SubagentTool {
    /// Create a new SubagentTool
    pub fn new(agent: Arc<UnifiedAgent>, name: String, description: String) -> Self {
        Self {
            agent,
            name,
            description,
        }
    }

    /// Get a reference to the wrapped agent
    pub fn agent(&self) -> &Arc<UnifiedAgent> {
        &self.agent
    }
}

#[async_trait]
impl Tool for SubagentTool {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn description(&self) -> String {
        self.description.clone()
    }

    fn parameters(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "input": {
                    "type": "string",
                    "description": format!("Input for the {} subagent", self.name)
                }
            },
            "required": ["input"]
        })
    }

    async fn run(&self, _input: Value) -> Result<String, crate::error::ToolError> {
        // This method is not used when requires_runtime() returns true
        // But we need to implement it to satisfy the trait
        Err(crate::error::ToolError::ConfigurationError(
            "SubagentTool requires runtime. Use run_with_runtime instead.".to_string(),
        ))
    }

    async fn run_with_runtime(
        &self,
        input: Value,
        _runtime: &ToolRuntime,
    ) -> Result<ToolResult, Box<dyn Error>> {
        // Extract input string from the JSON value
        let input_str = if let Some(input_val) = input.get("input") {
            if input_val.is_string() {
                input_val.as_str().unwrap().to_string()
            } else {
                serde_json::to_string(input_val)?
            }
        } else if input.is_string() {
            input.as_str().unwrap().to_string()
        } else {
            serde_json::to_string(&input)?
        };

        // Create a message from the input
        let message = Message::new_human_message(input_str);

        // Invoke the subagent
        let result = self
            .agent
            .invoke_messages(vec![message])
            .await
            .map_err(|e| format!("Subagent execution error: {}", e))?;

        Ok(ToolResult::Text(result))
    }

    fn requires_runtime(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::create_agent;

    #[tokio::test]
    async fn test_subagent_tool_creation() {
        let agent = Arc::new(create_agent("gpt-4o-mini", &[], Some("Test agent"), None).unwrap());

        let tool = SubagentTool::new(agent, "test_agent".to_string(), "A test agent".to_string());

        assert_eq!(tool.name(), "test_agent");
        assert_eq!(tool.description(), "A test agent");
        assert!(tool.requires_runtime());
    }
}
