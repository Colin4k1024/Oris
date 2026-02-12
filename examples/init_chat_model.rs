use oris::language_models::init_chat_model;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Example 1: Simple model initialization
    println!("Example 1: Simple model initialization");
    let model = init_chat_model("gpt-4o-mini", None, None, None, None, None, None, None).await?;

    let response = model.invoke("Say hello in one sentence.").await?;
    println!("Response: {}\n", response);

    // Example 2: Model with parameters
    println!("Example 2: Model with temperature and max_tokens");
    let model = init_chat_model(
        "gpt-4o-mini",
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

    // Example 3: Using provider:model format
    println!("Example 3: Using provider:model format");
    let model = init_chat_model(
        "openai:gpt-4o-mini",
        Some(0.5),
        Some(200),
        None,
        None,
        None,
        None,
        None,
    )
    .await?;

    let response = model.invoke("What is Rust?").await?;
    println!("Response: {}\n", response);

    // Example 4: Claude model
    println!("Example 4: Claude model");
    let model = init_chat_model(
        "claude-3-5-sonnet-20240620",
        Some(0.8),
        Some(150),
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
