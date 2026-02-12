use async_trait::async_trait;
use oris::agent::{create_agent, Middleware, MiddlewareContext, MiddlewareError};
use oris::schemas::agent::AgentAction;
use oris::schemas::Message;
use std::sync::Arc;

/// Custom middleware that adds a prefix to all tool observations
struct PrefixMiddleware {
    prefix: String,
}

impl PrefixMiddleware {
    fn new(prefix: String) -> Self {
        Self { prefix }
    }
}

#[async_trait]
impl Middleware for PrefixMiddleware {
    async fn after_tool_call(
        &self,
        _action: &AgentAction,
        observation: &str,
        _context: &mut MiddlewareContext,
    ) -> Result<Option<String>, MiddlewareError> {
        // Add prefix to observation
        Ok(Some(format!("{} {}", self.prefix, observation)))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    // Create custom middleware
    let prefix_middleware = PrefixMiddleware::new("[PREFIX]".to_string());

    // Create agent with custom middleware
    let agent = create_agent(
        "gpt-4o-mini",
        &[],
        Some("You are a helpful assistant."),
        Some(vec![Arc::new(prefix_middleware)]),
    )?;

    // Invoke the agent
    let result = agent
        .invoke_messages(vec![Message::new_human_message("Hello, how are you?")])
        .await?;

    println!("Agent response: {}", result);
    Ok(())
}
