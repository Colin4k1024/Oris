use oris::{
    agent::create_agent_with_structured_output,
    schemas::structured_output::{StructuredOutputSchema, ToolStrategy},
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Example structured output schema for contact information.
#[derive(Serialize, Deserialize, JsonSchema, Debug)]
struct ContactInfo {
    /// The name of the person
    name: String,
    /// The email address
    email: String,
    /// The phone number
    phone: String,
}

impl StructuredOutputSchema for ContactInfo {}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create an agent with structured output using ToolStrategy
    let strategy = ToolStrategy::<ContactInfo>::new()
        .with_tool_message_content("Contact information extracted successfully!".to_string());

    let agent = create_agent_with_structured_output(
        "gpt-4o-mini",
        &[],
        Some("You are a helpful assistant that extracts contact information from text."),
        Some(Box::new(strategy)),
        None, // Middleware (optional)
    )?;

    // Use the agent to extract structured information
    let result = agent
        .invoke_messages(vec![
            oris::schemas::Message::new_human_message(
                "Extract contact info from: John Doe, [email protected], (555) 123-4567",
            ),
        ])
        .await?;

    println!("Agent response: {}", result);

    // Note: In a full implementation, the structured_response would be available
    // in the agent state after execution. This is a conceptual example.

    Ok(())
}
