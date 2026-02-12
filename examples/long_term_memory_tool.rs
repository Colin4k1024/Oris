// Example: Using long-term memory in tools
// This demonstrates how tools can read and write long-term memory

use std::collections::HashMap;
use std::sync::Arc;

use oris::{
    agent::create_agent,
    error::ToolError,
    tools::{
        EnhancedInMemoryStore, EnhancedToolStore, SimpleContext, StoreValue, ToolResult,
        ToolRuntime,
    },
};
use serde_json::json;

// Tool that reads user information from long-term memory
struct GetUserInfoTool;

#[async_trait::async_trait]
impl oris::tools::Tool for GetUserInfoTool {
    fn name(&self) -> String {
        "get_user_info".to_string()
    }

    fn description(&self) -> String {
        "Get user information from long-term memory. Returns stored user preferences, language, and other information.".to_string()
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {},
            "required": []
        })
    }

    async fn run(&self, _input: serde_json::Value) -> Result<String, ToolError> {
        Err(ToolError::ExecutionError(
            "This tool requires runtime. Use run_with_runtime instead.".to_string(),
        ))
    }

    async fn run_with_runtime(
        &self,
        _input: serde_json::Value,
        runtime: &ToolRuntime,
    ) -> Result<ToolResult, Box<dyn std::error::Error>> {
        let user_id = runtime
            .context()
            .user_id()
            .ok_or_else(|| ToolError::MissingInput("user_id is required in context".to_string()))?;

        // Get user info from store
        let user_info = runtime.store().get(&["users"], user_id).await;

        match user_info {
            Some(info) => Ok(ToolResult::Text(format!(
                "User information: {}",
                serde_json::to_string_pretty(&info)?
            ))),
            None => Ok(ToolResult::Text(format!(
                "No information found for user: {}",
                user_id
            ))),
        }
    }

    fn requires_runtime(&self) -> bool {
        true
    }
}

// Tool that saves user information to long-term memory
struct SaveUserInfoTool;

#[async_trait::async_trait]
impl oris::tools::Tool for SaveUserInfoTool {
    fn name(&self) -> String {
        "save_user_info".to_string()
    }

    fn description(&self) -> String {
        "Save user information to long-term memory. Use this when the user provides information about themselves like name, language preferences, or other details.".to_string()
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
                },
                "preferences": {
                    "type": "string",
                    "description": "Any additional user preferences"
                }
            },
            "required": []
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

        // Build user info object
        let mut user_info = HashMap::new();
        if let Some(name) = input.get("name").and_then(|v| v.as_str()) {
            user_info.insert("name".to_string(), json!(name));
        }
        if let Some(language) = input.get("language").and_then(|v| v.as_str()) {
            user_info.insert("language".to_string(), json!(language));
        }
        if let Some(preferences) = input.get("preferences").and_then(|v| v.as_str()) {
            user_info.insert("preferences".to_string(), json!(preferences));
        }

        let value = json!(user_info);

        // Add metadata
        let mut metadata = HashMap::new();
        metadata.insert("updated_at".to_string(), json!("2024-01-01T00:00:00Z"));

        // Save to store
        // Note: We need to cast to EnhancedInMemoryStore to use put_with_metadata
        // For this example, we'll use basic put
        runtime.store().put(&["users"], user_id, value).await;

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
    let _store: Arc<dyn oris::tools::ToolStore> =
        Arc::new(EnhancedInMemoryStore::new());

    // Pre-populate with some data
    // Note: We need to use EnhancedInMemoryStore directly to use enhanced features
    let enhanced_store = Arc::new(EnhancedInMemoryStore::new());
    let store: Arc<dyn oris::tools::ToolStore> = enhanced_store.clone();

    let mut metadata = HashMap::new();
    metadata.insert("created_at".to_string(), json!("2024-01-01"));

    enhanced_store
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

    // Create context with user_id
    let _context = Arc::new(SimpleContext::new().with_user_id("user_123".to_string()));

    // Create agent
    let _agent = create_agent(
        "gpt-4o-mini",
        &tools,
        Some("You are a helpful assistant. Use get_user_info to recall user information and save_user_info to remember new information."),
        None,
    )?;

    println!("Long-term Memory Tool Example\n");

    println!("Long-term Memory Tool Example\n");

    // Read from store
    let user_info = store.get(&["users"], "user_123").await;
    println!("Retrieved user info: {:?}", user_info);

    // Read with metadata using enhanced store
    let user_info_with_metadata = enhanced_store
        .get_with_metadata(&["users"], "user_123")
        .await;
    if let Some(info) = user_info_with_metadata {
        println!("Retrieved with metadata:");
        println!("  Value: {}", info.value);
        println!("  Metadata: {:?}", info.metadata);
    }

    Ok(())
}
