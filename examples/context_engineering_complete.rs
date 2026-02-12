// Example: Complete context engineering demonstration
// This shows how to combine multiple context engineering techniques

use std::sync::Arc;
use tokio::sync::Mutex;

use oris::{
    agent::{
        context_engineering::middleware::{
            DynamicToolsMiddleware, EnhancedDynamicPromptMiddleware, InjectionPosition,
            MessageInjectionMiddleware,
        },
        create_agent, AgentState,
    },
    schemas::messages::Message,
    tools::{InMemoryStore, SimpleContext},
};
use serde_json::json;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // env_logger::init(); // Uncomment if env_logger is available

    // 1. Dynamic prompt based on runtime context
    let dynamic_prompt = EnhancedDynamicPromptMiddleware::from_runtime(|runtime| {
        let user_id = runtime.context().user_id().unwrap_or("Guest");
        let user_role = runtime.context().get("user_role").unwrap_or("user");

        format!(
            "You are a helpful assistant for user {} (role: {}). \
             Provide accurate and helpful responses.",
            user_id, user_role
        )
    });

    // 2. Dynamic tools based on permissions
    let dynamic_tools = DynamicToolsMiddleware::from_permissions(|ctx| {
        let user_role = ctx.get("user_role").unwrap_or("viewer");

        match user_role {
            "admin" => vec![
                "read_tool".to_string(),
                "write_tool".to_string(),
                "delete_tool".to_string(),
            ],
            "editor" => vec!["read_tool".to_string(), "write_tool".to_string()],
            _ => vec!["read_tool".to_string()],
        }
    });

    // 3. Message injection for compliance rules
    let compliance_injector =
        MessageInjectionMiddleware::inject_compliance_rules(InjectionPosition::End);

    // Create state with custom data
    let mut state = AgentState::new();
    state.set_field("session_id".to_string(), json!("session_456"));

    // Create agent with all context engineering features
    let agent = create_agent(
        "gpt-4o-mini",
        &[],
        Some("Default prompt"), // Will be overridden by dynamic prompt
        Some(vec![
            Arc::new(dynamic_prompt),
            Arc::new(dynamic_tools),
            Arc::new(compliance_injector),
        ]),
    )?
    .with_context(Arc::new(
        SimpleContext::new()
            .with_user_id("user_123".to_string())
            .with_custom("user_role".to_string(), "admin".to_string())
            .with_custom("user_jurisdiction".to_string(), "US".to_string())
            .with_custom("compliance_frameworks".to_string(), "HIPAA".to_string())
            .with_custom("industry".to_string(), "healthcare".to_string()),
    ))
    .with_store(Arc::new(InMemoryStore::new()))
    .with_state(Arc::new(Mutex::new(state)));

    // Use the agent
    let result = agent
        .invoke_messages(vec![Message::new_human_message(
            "What are my available tools and compliance requirements?",
        )])
        .await?;

    println!("Agent response: {}", result);

    Ok(())
}
