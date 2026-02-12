// Example: Dynamic tool selection based on permissions
// This demonstrates how to filter tools based on user permissions or state

use std::sync::Arc;

use oris::{
    agent::{context_engineering::middleware::DynamicToolsMiddleware, create_agent},
    error::ToolError,
    schemas::messages::Message,
    tools::{SimpleContext, Tool},
};

// Example tools
struct PublicTool;
struct AdminTool;

#[async_trait::async_trait]
impl Tool for PublicTool {
    fn name(&self) -> String {
        "public_search".to_string()
    }
    fn description(&self) -> String {
        "Public search tool".to_string()
    }
    async fn run(&self, _input: serde_json::Value) -> Result<String, ToolError> {
        Ok("Public search result".to_string())
    }
}

#[async_trait::async_trait]
impl Tool for AdminTool {
    fn name(&self) -> String {
        "admin_delete".to_string()
    }
    fn description(&self) -> String {
        "Admin delete tool".to_string()
    }
    async fn run(&self, _input: serde_json::Value) -> Result<String, ToolError> {
        Ok("Deleted".to_string())
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // env_logger::init(); // Uncomment if env_logger is available

    // Create dynamic tools middleware based on permissions
    let dynamic_tools = DynamicToolsMiddleware::from_permissions(|ctx| {
        // Get user role from context
        let user_role = ctx.get("user_role").unwrap_or("viewer");

        match user_role {
            "admin" => vec!["public_search".to_string(), "admin_delete".to_string()],
            "editor" => vec!["public_search".to_string()],
            _ => vec!["public_search".to_string()], // Viewers only get public tools
        }
    });

    // Alternative: exclude specific tools
    let _exclude_tools = DynamicToolsMiddleware::exclude_tools(vec![
        "admin_delete".to_string(),
        "sensitive_tool".to_string(),
    ]);

    // Create agent with dynamic tools middleware
    let public_tool = Arc::new(PublicTool);
    let admin_tool = Arc::new(AdminTool);

    let agent = create_agent(
        "gpt-4o-mini",
        &[public_tool, admin_tool],
        Some("You are a helpful assistant"),
        Some(vec![Arc::new(dynamic_tools)]),
    )?
    .with_context(Arc::new(
        SimpleContext::new()
            .with_user_id("user_123".to_string())
            .with_custom("user_role".to_string(), "viewer".to_string()),
    ));

    // Use the agent
    let result = agent
        .invoke_messages(vec![Message::new_human_message("Hello")])
        .await?;

    println!("Agent response: {}", result);

    Ok(())
}
