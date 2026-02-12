// Example: Dynamic prompt based on State and Store
// This demonstrates how to generate system prompts dynamically based on context

use std::sync::Arc;

use oris::{
    agent::{context_engineering::middleware::EnhancedDynamicPromptMiddleware, create_agent},
    schemas::messages::Message,
    tools::{InMemoryStore, SimpleContext},
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // env_logger::init(); // Uncomment if env_logger is available

    // Create dynamic prompt middleware that reads from runtime context
    let dynamic_prompt = EnhancedDynamicPromptMiddleware::from_runtime(|runtime| {
        let user_id = runtime.context().user_id().unwrap_or("Guest");
        format!(
            "You are a personalized assistant for user {}. Be friendly and helpful.",
            user_id
        )
    });

    // Alternative: from store
    let _store_based_prompt = EnhancedDynamicPromptMiddleware::from_store(
        |_store: &dyn oris::tools::ToolStore,
         ctx: &dyn oris::tools::ToolContext| {
            // In a real implementation, you'd read user preferences from store
            let user_id = ctx.user_id().unwrap_or("unknown");
            format!("You are assisting user {}.", user_id)
        },
    );

    // Create agent with dynamic prompt middleware
    let agent = create_agent(
        "gpt-4o-mini",
        &[],
        Some("Default system prompt"), // This will be overridden by dynamic prompt
        Some(vec![Arc::new(dynamic_prompt)]),
    )?
    .with_context(Arc::new(
        SimpleContext::new().with_user_id("user_123".to_string()),
    ))
    .with_store(Arc::new(InMemoryStore::new()));

    // Use the agent
    let result = agent
        .invoke_messages(vec![Message::new_human_message("What's my name?")])
        .await?;

    println!("Agent response: {}", result);

    Ok(())
}
