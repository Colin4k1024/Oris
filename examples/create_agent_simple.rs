use std::sync::Arc;

use async_trait::async_trait;
use oris::{
    agent::create_agent, chain::Chain, error::ToolError, prompt_args, schemas::messages::Message,
    tools::Tool,
};
use serde_json::Value;

/// A simple weather tool for demonstration
struct WeatherTool;

#[async_trait]
impl Tool for WeatherTool {
    fn name(&self) -> String {
        "get_weather".to_string()
    }

    fn description(&self) -> String {
        "Get weather for a given city".to_string()
    }

    async fn run(&self, input: Value) -> Result<String, ToolError> {
        let city = input
            .get("input")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        Ok(format!("It's always sunny in {}!", city))
    }
}

#[tokio::main]
async fn main() {
    // Create an agent with a simple API
    let agent = create_agent(
        "gpt-4o-mini",                       // Model as string - auto-detected
        &[Arc::new(WeatherTool)],            // Tools
        Some("You are a helpful assistant"), // System prompt
        None,                                // Middleware (optional)
    )
    .expect("Failed to create agent");

    // Use message-based input format (like LangChain Python)
    let result = agent
        .invoke(prompt_args! {
            "messages" => vec![
                Message::new_human_message("what is the weather in sf")
            ]
        })
        .await
        .expect("Failed to invoke agent");

    println!("Result: {}", result);

    // Also supports traditional prompt_args format (backward compatible)
    let result2 = agent
        .invoke(prompt_args! {
            "input" => "what is the weather in nyc"
        })
        .await
        .expect("Failed to invoke agent");

    println!("Result 2: {}", result2);

    // Use invoke_messages for direct message input
    let messages = vec![Message::new_human_message("what is the weather in paris")];
    let result3 = agent
        .invoke_messages(messages)
        .await
        .expect("Failed to invoke agent");

    println!("Result 3: {}", result3);
}
