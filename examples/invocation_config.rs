use oris::language_models::{init_chat_model, InvocationConfig};
use serde_json::json;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize a model
    let model = init_chat_model(
        "gpt-4o-mini",
        Some(0.7),
        Some(500),
        None,
        None,
        None,
        None,
        None,
    )
    .await?;

    // Create invocation config with metadata
    let config = InvocationConfig::new()
        .with_run_name("production_run_001".to_string())
        .with_tags(vec!["production".to_string(), "user_query".to_string()])
        .add_metadata("user_id".to_string(), json!("user_123"))
        .add_metadata("session_id".to_string(), json!("session_456"))
        .add_metadata("request_id".to_string(), json!("req_789"))
        .with_max_concurrency(5)
        .with_recursion_limit(10);

    println!("Invocation Config:");
    println!("  Run name: {:?}", config.run_name);
    println!("  Tags: {:?}", config.tags);
    println!("  Metadata: {:?}", config.metadata);
    println!("  Max concurrency: {:?}", config.max_concurrency);
    println!("  Recursion limit: {:?}\n", config.recursion_limit);

    // Use the model with config (conceptual - actual implementation
    // would pass config to generate/invoke methods)
    let response = model
        .invoke_with_config("What is the capital of France?", Some(&config))
        .await?;

    println!("Response: {}", response);

    // Example: Merging configs
    let mut base_config = InvocationConfig::new()
        .with_tags(vec!["base".to_string()])
        .add_metadata("base_key".to_string(), json!("base_value"));

    let override_config = InvocationConfig::new()
        .with_tags(vec!["override".to_string()])
        .add_metadata("override_key".to_string(), json!("override_value"));

    base_config.merge(override_config);

    println!("\nMerged config:");
    println!("  Tags: {:?}", base_config.tags);
    println!("  Metadata: {:?}", base_config.metadata);

    Ok(())
}
