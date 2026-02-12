use oris::agent::{
    create_agent, ContentFilterMiddleware, HumanInTheLoopMiddleware, PIIMiddleware, PIIStrategy,
    PIIType,
};
use oris::schemas::Message;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    // Layer 1: Deterministic input filter (before agent)
    let content_filter = ContentFilterMiddleware::new().with_banned_keywords(vec![
        "hack".to_string(),
        "exploit".to_string(),
        "malware".to_string(),
    ]);

    // Layer 2: PII protection (before and after model)
    let email_redact = PIIMiddleware::new(PIIType::Email, PIIStrategy::Redact)
        .with_apply_to_input(true)
        .with_apply_to_output(true);

    // Layer 3: Human approval for sensitive operations
    let human_approval = HumanInTheLoopMiddleware::new()
        .with_interrupt_on("send_email".to_string(), true)
        .with_interrupt_on("delete_database".to_string(), true)
        .with_interrupt_on("search".to_string(), false); // Auto-approve safe operations

    // Create agent with multiple guardrails
    let agent = create_agent(
        "gpt-4o-mini",
        &[],
        Some("You are a helpful assistant with safety measures in place."),
        Some(vec![
            Arc::new(content_filter),
            Arc::new(email_redact),
            Arc::new(human_approval),
        ]),
    )?;

    println!("Testing combined guardrails...");

    // Test 1: Content filter should block banned keywords
    println!("\n1. Testing content filter...");
    let result = agent
        .invoke_messages(vec![Message::new_human_message(
            "How do I hack into a system?",
        )])
        .await;

    match result {
        Ok(_) => println!("Unexpected: Banned content was not blocked"),
        Err(e) => println!("Expected: Content filter blocked request - {}", e),
    }

    // Test 2: PII should be redacted
    println!("\n2. Testing PII redaction...");
    let result = agent
        .invoke_messages(vec![Message::new_human_message(
            "My email is [email protected]",
        )])
        .await?;

    println!("Result (should have redacted email): {}", result);

    // Test 3: Normal request should work
    println!("\n3. Testing normal request...");
    let result = agent
        .invoke_messages(vec![Message::new_human_message(
            "What is the capital of France?",
        )])
        .await?;

    println!("Result: {}", result);

    Ok(())
}
