use oris::agent::{create_agent, PIIMiddleware, PIIStrategy, PIIType};
use oris::schemas::Message;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    // Create PII middleware to redact emails in input
    let email_redact = PIIMiddleware::new(PIIType::Email, PIIStrategy::Redact)
        .with_apply_to_input(true)
        .with_apply_to_output(true);

    // Create PII middleware to mask credit cards in input
    let credit_card_mask =
        PIIMiddleware::new(PIIType::CreditCard, PIIStrategy::Mask).with_apply_to_input(true);

    // Create PII middleware to block API keys
    let api_key_block = PIIMiddleware::with_custom_pattern(
        PIIType::Custom("API_KEY".to_string()),
        PIIStrategy::Block,
        r"sk-[a-zA-Z0-9]{32}",
    )?
    .with_apply_to_input(true);

    // Create agent with PII protection middleware
    let agent = create_agent(
        "gpt-4o-mini",
        &[],
        Some("You are a helpful assistant. Be careful with sensitive information."),
        Some(vec![
            Arc::new(email_redact),
            Arc::new(credit_card_mask),
            Arc::new(api_key_block),
        ]),
    )?;

    // Test with PII in the input
    println!("Testing PII protection...");

    // This should have email redacted
    let result = agent
        .invoke_messages(vec![Message::new_human_message(
            "My email is [email protected]",
        )])
        .await?;

    println!("Result with email: {}", result);

    Ok(())
}
