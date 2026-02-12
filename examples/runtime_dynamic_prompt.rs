// Example: Dynamic prompt based on runtime context
// This demonstrates how to generate system prompts dynamically based on user context

use std::sync::Arc;

use oris::{
    agent::{create_agent, DynamicPromptMiddleware},
    schemas::messages::Message,
    tools::{SimpleContext, ToolContext},
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();

    // Create dynamic prompt middleware
    let dynamic_prompt = DynamicPromptMiddleware::with_template(
        "You are a helpful assistant. Address the user as {user_name}. \
         The user ID is {user_id}."
            .to_string(),
    );

    // Alternative: custom prompt generator
    let _custom_prompt = DynamicPromptMiddleware::new(|ctx: &dyn ToolContext| {
        let user_id = ctx.user_id().unwrap_or("Guest");
        format!(
            "You are a personalized assistant for user {}. Be friendly and helpful.",
            user_id
        )
    });

    // Create agent with dynamic prompt middleware
    // DynamicPromptMiddleware implements Middleware trait
    let middleware: Vec<Arc<dyn oris::agent::Middleware>> =
        vec![Arc::new(dynamic_prompt)];

    let agent = create_agent(
        "gpt-4o-mini",
        &[],
        Some("Default system prompt"), // This will be overridden by dynamic prompt
        Some(middleware),
    )?
    .with_context(Arc::new(
        SimpleContext::new()
            .with_user_id("user_123".to_string())
            .with_custom("user_name".to_string(), "John".to_string()),
    ));

    // Use the agent
    let result = agent
        .invoke_messages(vec![Message::new_human_message("What's my name?")])
        .await?;

    println!("Agent response: {}", result);

    Ok(())
}
