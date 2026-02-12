use async_trait::async_trait;
use oris::agent::{create_agent, Middleware, MiddlewareContext, MiddlewareError};
use oris::prompt::PromptArgs;
use oris::schemas::agent::AgentAction;
use oris::schemas::Message;
use std::sync::Arc;

/// Custom guardrail: Block requests with excessive length
struct LengthGuardrail {
    max_length: usize,
}

impl LengthGuardrail {
    fn new(max_length: usize) -> Self {
        Self { max_length }
    }
}

#[async_trait]
impl Middleware for LengthGuardrail {
    async fn before_agent_plan(
        &self,
        input: &PromptArgs,
        _steps: &[(AgentAction, String)],
        _context: &mut MiddlewareContext,
    ) -> Result<Option<PromptArgs>, MiddlewareError> {
        // Extract input text
        let input_text = input.get("input").and_then(|v| v.as_str()).unwrap_or("");

        if input_text.len() > self.max_length {
            return Err(MiddlewareError::ValidationError(format!(
                "Input too long: {} characters (max: {})",
                input_text.len(),
                self.max_length
            )));
        }

        Ok(None)
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    // Create custom guardrail
    let length_guardrail = LengthGuardrail::new(1000);

    // Create agent with custom guardrail
    let agent = create_agent(
        "gpt-4o-mini",
        &[],
        Some("You are a helpful assistant."),
        Some(vec![Arc::new(length_guardrail)]),
    )?;

    // Test with normal input
    println!("Testing with normal input...");
    let result = agent
        .invoke_messages(vec![Message::new_human_message("Hello, how are you?")])
        .await?;

    println!("Result: {}", result);

    // Test with overly long input (should be blocked)
    println!("\nTesting with overly long input...");
    let long_input = "x".repeat(2000);
    let result = agent
        .invoke_messages(vec![Message::new_human_message(long_input)])
        .await;

    match result {
        Ok(_) => println!("Unexpected: Request was not blocked"),
        Err(e) => println!("Expected error: {}", e),
    }

    Ok(())
}
