#[cfg(feature = "bedrock")]
use oris::language_models::init_chat_model;

#[tokio::main]
#[cfg(feature = "bedrock")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Example 1: Simple model initialization (Claude)
    println!("Example 1: Simple model initialization (Claude)");
    let model = init_chat_model(
        "anthropic.claude-3-5-sonnet-20240620-v1:0",
        None,
        None,
        None,
        None,
        None,
        None,
        None,
    )
    .await?;

    let response = model.invoke("Say hello in one sentence.").await?;
    println!("Response: {}\n", response);

    // Example 2: With temperature and max_tokens
    println!("Example 2: With temperature and max_tokens");
    let model = init_chat_model(
        "anthropic.claude-3-5-sonnet-20240620-v1:0",
        Some(0.7),
        Some(100),
        None,
        None,
        None,
        None,
        None,
    )
    .await?;

    let response = model.invoke("Count to 5.").await?;
    println!("Response: {}\n", response);

    // Example 3: Using provider prefix
    println!("Example 3: Using provider prefix");
    let model = init_chat_model(
        "bedrock:anthropic.claude-3-5-sonnet-20240620-v1:0",
        None,
        None,
        None,
        None,
        None,
        None,
        None,
    )
    .await?;

    let response = model.invoke("What is Rust?").await?;
    println!("Response: {}\n", response);

    // Example 4: Llama model
    println!("Example 4: Llama model");
    let model = init_chat_model(
        "meta.llama3-70b-instruct-v1:0",
        Some(0.5),
        Some(200),
        None,
        None,
        None,
        None,
        None,
    )
    .await?;

    let response = model.invoke("Explain quantum computing briefly.").await?;
    println!("Response: {}\n", response);

    Ok(())
}

#[cfg(not(feature = "bedrock"))]
fn main() {
    println!("This example requires the 'bedrock' feature to be enabled.");
    println!("Run with: cargo run --example llm_bedrock --features bedrock");
    println!("\nNote: You also need AWS credentials configured.");
    println!("Set AWS_ACCESS_KEY_ID and AWS_SECRET_ACCESS_KEY environment variables,");
    println!("or configure AWS credentials using aws configure.");
}
