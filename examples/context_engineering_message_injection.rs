// Example: Message injection based on context
// This demonstrates how to inject file context, compliance rules, etc.

use std::sync::Arc;
use tokio::sync::Mutex;

use oris::{
    agent::{
        context_engineering::middleware::{InjectionPosition, MessageInjectionMiddleware},
        create_agent, AgentState,
    },
    schemas::messages::Message,
    tools::{InMemoryStore, SimpleContext},
};
use serde_json::json;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // env_logger::init(); // Uncomment if env_logger is available

    // Create message injection middleware for file context
    let file_context_injector =
        MessageInjectionMiddleware::inject_file_context(InjectionPosition::End);

    // Create message injection middleware for compliance rules
    let compliance_injector =
        MessageInjectionMiddleware::inject_compliance_rules(InjectionPosition::End);

    // Create state with uploaded files
    let mut state = AgentState::new();
    state.set_field(
        "uploaded_files".to_string(),
        json!([
            {
                "name": "project_proposal.pdf",
                "type": "pdf",
                "summary": "Q4 project proposal document"
            },
            {
                "name": "budget.xlsx",
                "type": "excel",
                "summary": "Budget spreadsheet for Q4"
            }
        ]),
    );

    // Create agent with message injection middleware
    let agent = create_agent(
        "gpt-4o-mini",
        &[],
        Some("You are a helpful assistant"),
        Some(vec![
            Arc::new(file_context_injector),
            Arc::new(compliance_injector),
        ]),
    )?
    .with_context(Arc::new(
        SimpleContext::new()
            .with_user_id("user_123".to_string())
            .with_custom("user_jurisdiction".to_string(), "EU".to_string())
            .with_custom("compliance_frameworks".to_string(), "GDPR".to_string())
            .with_custom("industry".to_string(), "finance".to_string()),
    ))
    .with_store(Arc::new(InMemoryStore::new()))
    .with_state(Arc::new(Mutex::new(state)));

    // Use the agent
    let result = agent
        .invoke_messages(vec![Message::new_human_message(
            "What files do I have access to?",
        )])
        .await?;

    println!("Agent response: {}", result);

    Ok(())
}
