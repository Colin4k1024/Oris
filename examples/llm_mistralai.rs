#[cfg(feature = "mistralai")]
use oris::language_models::init_chat_model;

#[tokio::main]
#[cfg(feature = "mistralai")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Example 1: Simple model initialization
    println!("Example 1: Simple model initialization");
    let model = init_chat_model(
        "mistral-small-latest",
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
        "mistral-medium-latest",
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
        "mistralai:mistral-large-latest",
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

    // Example 4: With API key override
    println!("Example 4: With API key override");
    let api_key =
        std::env::var("MISTRAL_API_KEY").expect("MISTRAL_API_KEY environment variable not set");
    let model = init_chat_model(
        "mistral-small-latest",
        Some(0.5),
        Some(200),
        None,
        None,
        Some(api_key),
        None,
        None,
    )
    .await?;

    let response = model.invoke("Explain quantum computing briefly.").await?;
    println!("Response: {}\n", response);

    Ok(())
}

#[cfg(not(feature = "mistralai"))]
fn main() {
    println!("This example requires the 'mistralai' feature to be enabled.");
    println!("Run with: cargo run --example llm_mistralai --features mistralai");
}
