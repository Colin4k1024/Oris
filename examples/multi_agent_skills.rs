use std::sync::Arc;

use oris::{
    agent::{create_agent, SimpleSkill, SkillAgentBuilder},
    schemas::messages::Message,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    env_logger::init();

    // Create base agent
    let base_agent = Arc::new(create_agent(
        "gpt-4o-mini",
        &[],
        Some("You are a helpful assistant with access to specialized knowledge."),
        None,
    )?);

    // Create skills
    let rust_skill = Arc::new(SimpleSkill::with_context(
        "rust_programming".to_string(),
        "Knowledge about Rust programming language".to_string(),
        "Rust is a systems programming language focused on safety and performance. \
         Key concepts: ownership, borrowing, lifetimes, traits, enums, pattern matching."
            .to_string(),
    ));

    let python_skill = Arc::new(SimpleSkill::with_context(
        "python_programming".to_string(),
        "Knowledge about Python programming language".to_string(),
        "Python is a high-level interpreted language. \
         Key features: dynamic typing, indentation-based syntax, extensive standard library."
            .to_string(),
    ));

    // Create skill agent
    let skill_agent = SkillAgentBuilder::new()
        .with_agent(base_agent)
        .with_skill(rust_skill)
        .with_skill(python_skill)
        .with_system_prompt_template(
            "You are a helpful assistant. Use the following knowledge when relevant:\n\n{skills}",
        )
        .build()?;

    println!("Testing Skills pattern...\n");

    // Test: Ask about Rust
    println!("Question: Explain Rust's ownership system");
    let response = skill_agent
        .invoke_messages(vec![Message::new_human_message(
            "Explain Rust's ownership system",
        )])
        .await?;
    println!("Response: {}\n", response);

    // Test: Ask about Python
    println!("Question: What are Python decorators?");
    let response = skill_agent
        .invoke_messages(vec![Message::new_human_message(
            "What are Python decorators?",
        )])
        .await?;
    println!("Response: {}\n", response);

    Ok(())
}
