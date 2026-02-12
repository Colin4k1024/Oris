//! Deep Agent with long-term memory: files under /memories/ persist in the store across turns.
//!
//! Aligned with [Long-term memory](https://docs.langchain.com/oss/python/deepagents/long-term-memory).
//! Uses [DeepAgentConfig::with_long_term_memory] so paths under the prefix are stored in the
//! ToolStore; a second invoke can read what the first wrote.
//!
//! Run with:
//! ```bash
//! cargo run --example deep_agent_long_term_memory
//! ```

use std::sync::Arc;

use oris::{
    agent::{create_deep_agent, DeepAgentConfig},
    chain::Chain,
    prompt_args,
    schemas::messages::Message,
    tools::{InMemoryStore, ToolStore},
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();

    let store: Arc<dyn ToolStore> = Arc::new(InMemoryStore::new());

    let config = DeepAgentConfig::new()
        .with_planning(false)
        .with_filesystem(true)
        .with_store(Arc::clone(&store))
        .with_long_term_memory("/memories/");

    let agent = create_deep_agent(
        "gpt-4o-mini",
        &[],
        Some(
            "You are a helpful assistant. When the user asks you to save preferences or facts, \
             write them to /memories/ (e.g. /memories/preferences.txt). You can read from /memories/ \
             in later turns to recall what was saved.",
        ),
        config,
    )?;

    println!("=== Long-term memory: first turn (save) ===\n");

    let _ = agent
        .invoke(prompt_args! {
            "messages" => vec![
                Message::new_human_message("Save to /memories/preferences.txt that my favorite color is blue and I prefer short answers.")
            ]
        })
        .await?;

    println!("\n=== Long-term memory: second turn (recall) ===\n");

    let result = agent
        .invoke(prompt_args! {
            "messages" => vec![
                Message::new_human_message("What did I tell you my favorite color was? Check /memories/preferences.txt if needed.")
            ]
        })
        .await?;

    println!("Response: {}\n", result);

    Ok(())
}
