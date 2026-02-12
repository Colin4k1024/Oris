use std::sync::Arc;

use async_trait::async_trait;
use oris::{
    agent::create_agent,
    chain::Chain,
    error::ToolError,
    prompt_args,
    tools::{Tool, ToolResult, ToolRuntime},
};
use serde_json::Value;

/// Example of a tool with custom parameters schema
struct WeatherTool;

#[async_trait]
impl Tool for WeatherTool {
    fn name(&self) -> String {
        "get_weather".to_string()
    }

    fn description(&self) -> String {
        "Get current weather for a location. Supports celsius and fahrenheit units.".to_string()
    }

    fn parameters(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "location": {
                    "type": "string",
                    "description": "City name or coordinates"
                },
                "units": {
                    "type": "string",
                    "enum": ["celsius", "fahrenheit"],
                    "default": "celsius",
                    "description": "Temperature unit preference"
                },
                "include_forecast": {
                    "type": "boolean",
                    "default": false,
                    "description": "Include 5-day forecast"
                }
            },
            "required": ["location"]
        })
    }

    async fn run(&self, input: Value) -> Result<String, ToolError> {
        let location = input["location"]
            .as_str()
            .ok_or_else(|| ToolError::MissingInput("location".to_string()))?;
        let units = input["units"].as_str().unwrap_or("celsius");
        let include_forecast = input["include_forecast"].as_bool().unwrap_or(false);

        let temp = if units == "celsius" { 22 } else { 72 };
        let unit_symbol = if units == "celsius" { "°C" } else { "°F" };

        let mut result = format!("Current weather in {}: {} {}", location, temp, unit_symbol);

        if include_forecast {
            result.push_str("\nNext 5 days: Sunny, 20-25°C");
        }

        Ok(result)
    }
}

/// Example tool with streaming updates
struct LongRunningTool;

#[async_trait]
impl Tool for LongRunningTool {
    fn name(&self) -> String {
        "process_data".to_string()
    }

    fn description(&self) -> String {
        "Process data with progress updates".to_string()
    }

    fn requires_runtime(&self) -> bool {
        true
    }

    async fn run(&self, _input: Value) -> Result<String, ToolError> {
        Ok("This tool requires runtime for streaming".to_string())
    }

    async fn run_with_runtime(
        &self,
        input: Value,
        runtime: &ToolRuntime,
    ) -> Result<ToolResult, Box<dyn std::error::Error>> {
        let task = input["task"].as_str().unwrap_or("default");

        // Stream progress updates
        runtime.stream(&format!("Starting task: {}", task));
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        runtime.stream("Processing step 1/3...");
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        runtime.stream("Processing step 2/3...");
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        runtime.stream("Processing step 3/3...");
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        runtime.stream("Task completed!");

        Ok(ToolResult::text(format!("Completed task: {}", task)))
    }
}

#[tokio::main]
async fn main() {
    let agent = create_agent(
        "gpt-4o-mini",
        &[Arc::new(WeatherTool), Arc::new(LongRunningTool)],
        Some("You are a helpful assistant"),
        None, // Middleware (optional)
    )
    .expect("Failed to create agent");

    // Test weather tool with advanced parameters
    let result = agent
        .invoke(prompt_args! {
            "input" => "What's the weather in San Francisco in fahrenheit with forecast?"
        })
        .await
        .expect("Failed to invoke agent");

    println!("Weather Result: {}", result);

    // Test long-running tool with streaming
    let result2 = agent
        .invoke(prompt_args! {
            "input" => "Process data for task 'analyze_reports'"
        })
        .await
        .expect("Failed to invoke agent");

    println!("Processing Result: {}", result2);
}
