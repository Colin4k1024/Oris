use std::sync::Arc;

use oris::{
    agent::{create_agent, HandoffAgentBuilder},
    schemas::messages::Message,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    env_logger::init();

    // Create specialized agents
    let customer_service_agent = Arc::new(create_agent(
        "gpt-4o-mini",
        &[],
        Some("You are a customer service agent. Help customers with their inquiries."),
        None,
    )?);

    let technical_support_agent = Arc::new(create_agent(
        "gpt-4o-mini",
        &[],
        Some("You are a technical support agent. Help with technical issues and troubleshooting."),
        None,
    )?);

    // Create handoff system
    let handoff_system = HandoffAgentBuilder::new()
        .with_base_agent(customer_service_agent.clone())
        .with_handoff_agent(
            "technical_support".to_string(),
            technical_support_agent.clone(),
        )
        .build()?;

    println!("Testing Handoffs pattern...\n");

    // Test: Customer service can hand off to technical support
    println!("Question: I'm having trouble connecting to the database");
    let response = handoff_system
        .invoke_messages(vec![Message::new_human_message(
            "I'm having trouble connecting to the database",
        )])
        .await?;
    println!("Response: {}\n", response);

    Ok(())
}
