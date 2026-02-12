use oris::{
    agent::create_agent_with_structured_output,
    schemas::structured_output::{ProviderStrategy, StructuredOutputSchema},
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Example structured output schema for product review analysis.
#[derive(Serialize, Deserialize, JsonSchema, Debug)]
struct ProductReview {
    /// The rating of the product (1-5)
    rating: Option<i32>,
    /// The sentiment of the review
    sentiment: String,
    /// Key points from the review
    key_points: Vec<String>,
}

impl StructuredOutputSchema for ProductReview {}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create an agent with structured output using ProviderStrategy
    // This uses the model provider's native structured output capabilities
    let strategy = ProviderStrategy::<ProductReview>::new().with_strict(true); // Enable strict schema adherence

    let agent = create_agent_with_structured_output(
        "gpt-4o-mini",
        &[],
        Some("You are a helpful assistant that analyzes product reviews."),
        Some(Box::new(strategy)),
        None, // Middleware (optional)
    )?;

    // Analyze a product review
    let result = agent
        .invoke_messages(vec![oris::schemas::Message::new_human_message(
            "Analyze this review: 'Great product: 5 out of 5 stars. Fast shipping, but expensive'",
        )])
        .await?;

    println!("Agent response: {}", result);

    // Note: With ProviderStrategy, the model provider (e.g., OpenAI) enforces
    // the schema natively, providing higher reliability.

    Ok(())
}
