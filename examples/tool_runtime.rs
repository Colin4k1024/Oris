use std::sync::Arc;

use async_trait::async_trait;
use oris::{
    agent::{create_agent_with_runtime, Command},
    chain::Chain,
    error::ToolError,
    prompt_args,
    tools::{InMemoryStore, SimpleContext, Tool, ToolResult, ToolRuntime},
};
use serde_json::{json, Value};

/// Example tool that accesses runtime state
struct StateAwareTool;

#[async_trait]
impl Tool for StateAwareTool {
    fn name(&self) -> String {
        "get_conversation_summary".to_string()
    }

    fn description(&self) -> String {
        "Get a summary of the current conversation".to_string()
    }

    fn requires_runtime(&self) -> bool {
        true
    }

    async fn run(&self, _input: Value) -> Result<String, ToolError> {
        Ok("This tool requires runtime".to_string())
    }

    async fn run_with_runtime(
        &self,
        _input: Value,
        runtime: &ToolRuntime,
    ) -> Result<ToolResult, Box<dyn std::error::Error>> {
        let state = runtime.state().await;
        let messages = &state.messages;

        let human_count = messages
            .iter()
            .filter(|m| {
                matches!(
                    m.message_type,
                    oris::schemas::MessageType::HumanMessage
                )
            })
            .count();
        let ai_count = messages
            .iter()
            .filter(|m| {
                matches!(
                    m.message_type,
                    oris::schemas::MessageType::AIMessage
                )
            })
            .count();

        let summary = format!(
            "Conversation has {} user messages and {} AI responses",
            human_count, ai_count
        );

        Ok(ToolResult::text(summary))
    }
}

/// Example tool that uses context
struct ContextAwareTool;

#[async_trait]
impl Tool for ContextAwareTool {
    fn name(&self) -> String {
        "get_user_info".to_string()
    }

    fn description(&self) -> String {
        "Get information about the current user".to_string()
    }

    fn requires_runtime(&self) -> bool {
        true
    }

    async fn run(&self, _input: Value) -> Result<String, ToolError> {
        Ok("This tool requires runtime".to_string())
    }

    async fn run_with_runtime(
        &self,
        _input: Value,
        runtime: &ToolRuntime,
    ) -> Result<ToolResult, Box<dyn std::error::Error>> {
        let context = runtime.context();
        let user_id = context.user_id().unwrap_or("unknown");
        let session_id = context.session_id().unwrap_or("none");

        Ok(ToolResult::text(format!(
            "User ID: {}, Session ID: {}",
            user_id, session_id
        )))
    }
}

/// Example tool that uses store
struct StoreAwareTool;

#[async_trait]
impl Tool for StoreAwareTool {
    fn name(&self) -> String {
        "save_preference".to_string()
    }

    fn description(&self) -> String {
        "Save a user preference to persistent storage".to_string()
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "key": {
                    "type": "string",
                    "description": "Preference key"
                },
                "value": {
                    "type": "string",
                    "description": "Preference value"
                }
            },
            "required": ["key", "value"]
        })
    }

    fn requires_runtime(&self) -> bool {
        true
    }

    async fn run(&self, _input: Value) -> Result<String, ToolError> {
        Ok("This tool requires runtime".to_string())
    }

    async fn run_with_runtime(
        &self,
        input: Value,
        runtime: &ToolRuntime,
    ) -> Result<ToolResult, Box<dyn std::error::Error>> {
        let key = input["key"]
            .as_str()
            .ok_or_else(|| ToolError::MissingInput("key".to_string()))?
            .to_string();
        let value = input["value"]
            .as_str()
            .ok_or_else(|| ToolError::MissingInput("value".to_string()))?
            .to_string();

        runtime
            .store()
            .put(&["preferences"], &key, json!(value))
            .await;

        Ok(ToolResult::text(format!(
            "Saved preference: {} = {}",
            key, value
        )))
    }
}

/// Example tool that updates state
struct StateUpdateTool;

#[async_trait]
impl Tool for StateUpdateTool {
    fn name(&self) -> String {
        "set_custom_field".to_string()
    }

    fn description(&self) -> String {
        "Set a custom field in the agent state".to_string()
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "field": {
                    "type": "string",
                    "description": "Field name"
                },
                "value": {
                    "type": "string",
                    "description": "Field value"
                }
            },
            "required": ["field", "value"]
        })
    }

    fn requires_runtime(&self) -> bool {
        true
    }

    async fn run(&self, _input: Value) -> Result<String, ToolError> {
        Ok("This tool requires runtime".to_string())
    }

    async fn run_with_runtime(
        &self,
        input: Value,
        runtime: &ToolRuntime,
    ) -> Result<ToolResult, Box<dyn std::error::Error>> {
        let field = input["field"]
            .as_str()
            .ok_or_else(|| ToolError::MissingInput("field".to_string()))?
            .to_string();
        let value = input["value"].clone();

        let mut state = runtime.state().await;
        state.set_field(field.clone(), value.clone());

        let command = Command::UpdateState {
            fields: {
                let mut fields = std::collections::HashMap::new();
                fields.insert(field, value);
                fields
            },
        };

        Ok(ToolResult::with_command(
            format!("Field set successfully"),
            command,
        ))
    }
}

#[tokio::main]
async fn main() {
    // Create context with user information
    let context = Arc::new(SimpleContext::new().with_user_id("user123".to_string()));

    // Create store for persistent data
    let store = Arc::new(InMemoryStore::new());

    // Create agent with runtime support
    let agent = create_agent_with_runtime(
        "gpt-4o-mini",
        &[
            Arc::new(StateAwareTool),
            Arc::new(ContextAwareTool),
            Arc::new(StoreAwareTool),
            Arc::new(StateUpdateTool),
        ],
        Some("You are a helpful assistant with access to runtime information"),
        Some(context),
        Some(store),
        None, // response_format
        None, // middleware
        None, // file_backend
    )
    .expect("Failed to create agent");

    // Use the agent
    let result = agent
        .invoke(prompt_args! {
            "input" => "What's my user ID and save a preference with key 'theme' and value 'dark'"
        })
        .await
        .expect("Failed to invoke agent");

    println!("Result: {}", result);
}
