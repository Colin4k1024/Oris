// Example: Basic long-term memory operations
// This demonstrates storing and retrieving values with metadata

use std::sync::Arc;

use oris::{
    agent::create_agent,
    error::ToolError,
    tools::{
        EnhancedInMemoryStore, EnhancedToolStore, StoreValue, ToolResult, ToolRuntime, ToolStore,
    },
};
use serde_json::json;

// A simple tool that reads user information from long-term memory
struct GetUserInfoTool;

#[async_trait::async_trait]
impl oris::tools::Tool for GetUserInfoTool {
    fn name(&self) -> String {
        "get_user_info".to_string()
    }

    fn description(&self) -> String {
        "Get user information from long-term memory. Use this when you need to recall information about a user.".to_string()
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "user_id": {
                    "type": "string",
                    "description": "The user ID to look up"
                }
            },
            "required": ["user_id"]
        })
    }

    async fn run(&self, _input: serde_json::Value) -> Result<String, ToolError> {
        Err(ToolError::ExecutionError(
            "This tool requires runtime. Use run_with_runtime instead.".to_string(),
        ))
    }

    async fn run_with_runtime(
        &self,
        input: serde_json::Value,
        runtime: &ToolRuntime,
    ) -> Result<ToolResult, Box<dyn std::error::Error>> {
        let user_id = input
            .get("user_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::MissingInput("user_id is required".to_string()))?;

        // Get user_id from context
        let context_user_id = runtime.context().user_id().unwrap_or(user_id);

        // Try to get from store using enhanced features
        // Note: In a real implementation, you'd need to downcast the store
        // For this example, we'll use the basic ToolStore interface
        let user_info = runtime.store().get(&["users"], context_user_id).await;

        match user_info {
            Some(info) => Ok(ToolResult::Text(format!("User info: {}", info))),
            None => Ok(ToolResult::Text(format!(
                "No information found for user: {}",
                context_user_id
            ))),
        }
    }

    fn requires_runtime(&self) -> bool {
        true
    }
}

// A tool that saves user information to long-term memory
struct SaveUserInfoTool;

#[async_trait::async_trait]
impl oris::tools::Tool for SaveUserInfoTool {
    fn name(&self) -> String {
        "save_user_info".to_string()
    }

    fn description(&self) -> String {
        "Save user information to long-term memory. Use this when the user provides information about themselves.".to_string()
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "The user's name"
                },
                "language": {
                    "type": "string",
                    "description": "The user's preferred language"
                }
            },
            "required": ["name"]
        })
    }

    async fn run(&self, _input: serde_json::Value) -> Result<String, ToolError> {
        Err(ToolError::ExecutionError(
            "This tool requires runtime. Use run_with_runtime instead.".to_string(),
        ))
    }

    async fn run_with_runtime(
        &self,
        input: serde_json::Value,
        runtime: &ToolRuntime,
    ) -> Result<ToolResult, Box<dyn std::error::Error>> {
        let user_id = runtime
            .context()
            .user_id()
            .ok_or_else(|| ToolError::MissingInput("user_id is required in context".to_string()))?;

        // Create store value with metadata
        let mut metadata = std::collections::HashMap::new();
        metadata.insert(
            "updated_at".to_string(),
            json!(chrono::Utc::now().to_rfc3339()),
        );

        let store_value = StoreValue::with_metadata(
            json!({
                "name": input.get("name").and_then(|v| v.as_str()).unwrap_or("Unknown"),
                "language": input.get("language").and_then(|v| v.as_str()).unwrap_or("English"),
            }),
            metadata,
        );

        // Save to store
        // Note: In a real implementation with EnhancedInMemoryStore, you'd use put_with_metadata
        // For this example, we'll use the basic interface
        runtime
            .store()
            .put(&["users"], user_id, store_value.value.clone())
            .await;

        Ok(ToolResult::Text(
            "User information saved successfully.".to_string(),
        ))
    }

    fn requires_runtime(&self) -> bool {
        true
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();

    // Create enhanced store
    let store = Arc::new(EnhancedInMemoryStore::new());

    // Pre-populate with some user data
    let mut metadata = std::collections::HashMap::new();
    metadata.insert("created_at".to_string(), json!("2024-01-01"));

    store
        .put_with_metadata(
            &["users"],
            "user_123",
            StoreValue::with_metadata(
                json!({
                    "name": "John Smith",
                    "language": "English",
                }),
                metadata,
            ),
        )
        .await;

    // Create tools
    let tools: Vec<Arc<dyn oris::tools::Tool>> =
        vec![Arc::new(GetUserInfoTool), Arc::new(SaveUserInfoTool)];

    // Create agent with store
    let _agent = create_agent(
        "gpt-4o-mini",
        &tools,
        Some("You are a helpful assistant that can remember user information."),
        None,
    )?;

    println!("Long-term Memory Basic Example\n");

    // Read from store
    let user_info = store.get_with_metadata(&["users"], "user_123").await;
    if let Some(info) = user_info {
        println!("Retrieved user info:");
        println!("  Value: {}", info.value);
        println!("  Metadata: {:?}", info.metadata);
    }

    // Write to store
    let new_info = StoreValue::new(json!({
        "name": "Jane Doe",
        "language": "Spanish",
    }));
    store
        .put_with_metadata(&["users"], "user_456", new_info)
        .await;

    println!("\nSaved new user info");

    // List keys in namespace
    let keys = store.list(&["users"]).await;
    println!("\nKeys in 'users' namespace: {:?}", keys);

    Ok(())
}
