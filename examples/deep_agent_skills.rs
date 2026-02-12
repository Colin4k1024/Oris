//! Deep Agent with directory-based skills (progressive disclosure).
//!
//! Aligned with [Deep Agent Skills](https://docs.langchain.com/oss/python/deepagents/skills).
//! Each skill is a directory containing `SKILL.md` with YAML frontmatter (name, description).
//! Only frontmatter is read at startup; when the user message matches a skill, its full
//! content is loaded and injected into context for that turn.
//!
//! Run with:
//! ```bash
//! cargo run --example deep_agent_skills
//! ```

use std::path::PathBuf;

use oris::{
    agent::{create_deep_agent, DeepAgentConfig},
    chain::Chain,
    prompt_args,
    schemas::messages::Message,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();

    // Path to skills directory (each subdir can contain SKILL.md with frontmatter)
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_string());
    let skills_dir: PathBuf = [&manifest_dir, "examples", "skills"].iter().collect();

    let config = DeepAgentConfig::new()
        .with_planning(true)
        .with_filesystem(false)
        .with_skill_dir(skills_dir);

    let agent = create_deep_agent(
        "gpt-4o-mini",
        &[],
        Some("You are a helpful assistant. Use Skills when they are injected."),
        config,
    )?;

    println!("=== Deep Agent with progressive skills ===\n");

    // This prompt should match the langgraph-docs skill (description mentions LangGraph)
    let result = agent
        .invoke(prompt_args! {
            "messages" => vec![
                Message::new_human_message("What is LangGraph? Do you have a skill loaded for it?")
            ]
        })
        .await?;

    println!("Response: {}\n", result);

    Ok(())
}
