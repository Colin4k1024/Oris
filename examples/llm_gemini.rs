#[cfg(feature = "gemini")]
use oris::language_models::init_chat_model;

#[tokio::main]
#[cfg(feature = "gemini")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Example 1: Simple model initialization
    println!("Example 1: Simple model initialization");
    let model =
        init_chat_model("gemini-1.5-flash", None, None, None, None, None, None, None).await?;

    let response = model.invoke("Say hello in one sentence.").await?;
    println!("Response: {}\n", response);

    // Example 2: With temperature and max_tokens
    println!("Example 2: With temperature and max_tokens");
    let model = init_chat_model(
        "gemini-1.5-pro",
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
        "gemini:gemini-2.0-flash",
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
        std::env::var("GOOGLE_API_KEY").expect("GOOGLE_API_KEY environment variable not set");
    let model = init_chat_model(
        "gemini-1.5-flash",
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

#[cfg(not(feature = "gemini"))]
fn main() {
    println!("This example requires the 'gemini' feature to be enabled.");
    println!("Run with: cargo run --example llm_gemini --features gemini");
}
