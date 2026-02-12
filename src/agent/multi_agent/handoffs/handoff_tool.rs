use std::error::Error;

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::{
    agent::state::Command,
    tools::{Tool, ToolResult, ToolRuntime},
};

/// A tool that enables handoff between agents.
///
/// When called, this tool updates the agent state to indicate which
/// agent should be active, enabling the Handoffs pattern.
pub struct HandoffTool {
    /// Name of the tool
    name: String,
    /// Description of what this tool does
    description: String,
}

impl HandoffTool {
    /// Create a new HandoffTool
    pub fn new() -> Self {
        Self {
            name: "handoff".to_string(),
            description: "Transfer control to another specialized agent. Use this when the current agent cannot handle the request and another agent would be better suited.".to_string(),
        }
    }

    /// Create with custom name and description
    pub fn with_name_and_description(name: String, description: String) -> Self {
        Self { name, description }
    }
}

impl Default for HandoffTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for HandoffTool {
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
                "agent_name": {
                    "type": "string",
                    "description": "The name of the agent to hand off control to"
                },
                "message": {
                    "type": "string",
                    "description": "Optional message to pass to the target agent"
                }
            },
            "required": ["agent_name"]
        })
    }

    async fn run(&self, _input: Value) -> Result<String, crate::error::ToolError> {
        // This method is not used when requires_runtime() returns true
        // But we need to implement it to satisfy the trait
        Err(crate::error::ToolError::ConfigurationError(
            "HandoffTool requires runtime. Use run_with_runtime instead.".to_string(),
        ))
    }

    async fn run_with_runtime(
        &self,
        input: Value,
        runtime: &ToolRuntime,
    ) -> Result<ToolResult, Box<dyn Error>> {
        // Parse the input to extract agent_name and optional message
        let agent_name = if let Some(name_val) = input.get("agent_name") {
            if name_val.is_string() {
                name_val.as_str().unwrap().to_string()
            } else {
                return Err("agent_name must be a string".into());
            }
        } else {
            return Err("agent_name is required".into());
        };

        let message = input
            .get("message")
            .and_then(|m| m.as_str())
            .map(|s| s.to_string());

        // Update the state to set the active agent
        let mut state = runtime.state().await;
        state.set_active_agent(agent_name.clone());

        // If there's a message, add it to the state's custom fields
        if let Some(msg) = message {
            state.set_field("handoff_message".to_string(), json!(msg));
        }

        // Create a command to update the state
        let mut fields = std::collections::HashMap::new();
        fields.insert("active_agent".to_string(), json!(agent_name));
        if let Some(msg) = input.get("message") {
            fields.insert("handoff_message".to_string(), msg.clone());
        }

        let command = Command::UpdateState { fields };

        Ok(ToolResult::with_command(
            format!("Handing off control to agent: {}", agent_name),
            command,
        ))
    }

    fn requires_runtime(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_handoff_tool_creation() {
        let tool = HandoffTool::new();
        assert_eq!(tool.name(), "handoff");
        assert!(tool.requires_runtime());
    }

    #[test]
    fn test_handoff_tool_custom() {
        let tool = HandoffTool::with_name_and_description(
            "transfer".to_string(),
            "Transfer control".to_string(),
        );
        assert_eq!(tool.name(), "transfer");
        assert_eq!(tool.description(), "Transfer control");
    }
}
