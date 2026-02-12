// Example: Using typed context with agents
// This demonstrates how to define and use type-safe context

use std::sync::Arc;

use oris::{
    agent::{create_agent, ContextAdapter, TypedContext, TypedContextFields},
    schemas::messages::Message,
    tools::ToolContext,
};

// Define a typed context
#[derive(Clone)]
struct MyContext {
    user_id: String,
    user_name: String,
    db_connection: Option<String>,
}

// Implement TypedContext
impl TypedContext for MyContext {
    fn to_tool_context(&self) -> Arc<dyn ToolContext> {
        Arc::new(ContextAdapter::new(self.clone()))
    }
}

// Implement TypedContextFields to enable field access
impl TypedContextFields for MyContext {
    fn user_id(&self) -> Option<&str> {
        Some(&self.user_id)
    }

    fn get(&self, key: &str) -> Option<&str> {
        match key {
            "user_name" => Some(&self.user_name),
            "db_connection" => self.db_connection.as_deref(),
            _ => None,
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();

    // Create typed context
    let context = MyContext {
        user_id: "user_123".to_string(),
        user_name: "John Smith".to_string(),
        db_connection: Some("postgresql://localhost".to_string()),
    };

    // Convert to ToolContext
    let tool_context = context.to_tool_context();
    println!("User ID: {:?}", tool_context.user_id());
    println!("User Name: {:?}", tool_context.get("user_name"));

    // Create agent with context
    let agent = create_agent(
        "gpt-4o-mini",
        &[],
        Some("You are a helpful assistant"),
        None,
    )?
    .with_context(tool_context);

    // Use the agent
    let result = agent
        .invoke_messages(vec![Message::new_human_message("What's my name?")])
        .await?;

    println!("Agent response: {}", result);

    Ok(())
}
