use oris::language_models::init_chat_model;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Example 1: Simple model initialization
    println!("Example 1: Simple model initialization");
    let model = init_chat_model(
        "microsoft/Phi-3-mini-4k-instruct",
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
        "microsoft/Phi-3-mini-4k-instruct",
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
        "huggingface:microsoft/Phi-3-mini-4k-instruct",
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
    if let Ok(api_key) = std::env::var("HUGGINGFACE_API_KEY") {
        let model = init_chat_model(
            "microsoft/Phi-3-mini-4k-instruct",
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
    } else {
        println!("HUGGINGFACE_API_KEY not set, skipping API key example");
    }

    Ok(())
}
