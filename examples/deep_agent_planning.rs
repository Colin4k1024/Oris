//! Deep Agent planning: write_todos and long-term memory (store).
//!
//! Demonstrates the **write_todos** tool and persistent store:
//! - Todos are stored in the tool store under namespace `["todos"]` keyed by session/thread
//! - The agent can break down tasks and update progress via write_todos
//!
//! Run with:
//! ```bash
//! cargo run --example deep_agent_planning
//! ```

use std::sync::Arc;

use oris::{
    agent::{create_deep_agent, DeepAgentConfig},
    chain::Chain,
    prompt_args,
    schemas::messages::Message,
    tools::InMemoryStore,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();

    // Optional: use a custom store so todos persist (here we use default InMemoryStore)
    let store = Arc::new(InMemoryStore::new());

    let config = DeepAgentConfig::new()
        .with_planning(true)
        .with_filesystem(false)
        .with_store(store);

    let agent = create_deep_agent(
        "gpt-4o-mini",
        &[],
        Some(
            "You have a write_todos tool. Use it to break complex tasks into steps and track progress. \
             When the user gives a multi-step task, first call write_todos with a list of steps (id, title, status: pending/done/cancelled), \
             then you can update them as you go.",
        ),
        config,
    )?;

    println!("=== Deep Agent planning (write_todos) ===\n");

    let result = agent
        .invoke(prompt_args! {
            "messages" => vec![
                Message::new_human_message(
                    "I need to plan a short essay: 1) Choose topic 2) Outline 3) Write draft. \
                     Use write_todos to create these three steps."
                )
            ]
        })
        .await?;

    println!("Response: {}\n", result);

    Ok(())
}
