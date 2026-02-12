use oris::language_models::{init_chat_model, ConfigurableModel, InvocationConfig};
use serde_json::json;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create a base model
    let base_model = init_chat_model(
        "gpt-4o-mini",
        Some(0.7),
        Some(1000),
        None,
        None,
        None,
        None,
        None,
    )
    .await?;

    // Wrap it in a ConfigurableModel
    let _configurable = ConfigurableModel::new(base_model)
        .with_configurable_fields(vec![
            "model".to_string(),
            "temperature".to_string(),
            "max_tokens".to_string(),
        ])
        .with_config_prefix("first".to_string());

    println!("Created configurable model with prefix 'first'");
    println!("Configurable fields: model, temperature, max_tokens\n");

    // Example: Using invocation config (conceptual - full implementation would
    // require runtime model switching which is complex)
    let config = InvocationConfig::new()
        .with_run_name("my_experiment".to_string())
        .with_tags(vec!["test".to_string(), "demo".to_string()])
        .add_metadata("user_id".to_string(), json!("123"))
        .add_metadata("experiment_id".to_string(), json!("exp_001"));

    println!("Invocation config created:");
    println!("  Run name: {:?}", config.run_name);
    println!("  Tags: {:?}", config.tags);
    println!("  Metadata: {:?}", config.metadata);

    // Note: Full configurable model functionality would require
    // runtime model creation based on config, which is a more advanced feature.
    // This example demonstrates the API structure.

    Ok(())
}
