use std::sync::Arc;

use oris::{
    agent::{create_agent, SubagentInfo, SubagentsBuilder},
    schemas::messages::Message,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    env_logger::init();

    // Create specialized subagents
    let math_agent = Arc::new(create_agent(
        "gpt-4o-mini",
        &[],
        Some("You are a math expert. Solve mathematical problems step by step."),
        None,
    )?);

    let coding_agent = Arc::new(create_agent(
        "gpt-4o-mini",
        &[],
        Some("You are a coding expert. Help with programming questions and write code."),
        None,
    )?);

    // Create main agent with subagents
    let main_agent = SubagentsBuilder::new()
        .with_model("gpt-4o-mini")
        .with_system_prompt(
            "You are a coordinator agent. You have access to specialized subagents. \
             Use them when appropriate to help the user.",
        )
        .with_subagent(SubagentInfo::new(
            math_agent,
            "math_agent".to_string(),
            "A specialized agent for solving mathematical problems".to_string(),
        ))
        .with_subagent(SubagentInfo::new(
            coding_agent,
            "coding_agent".to_string(),
            "A specialized agent for programming and coding questions".to_string(),
        ))
        .build()?;

    // Test the multi-agent system
    println!("Testing Subagents pattern...\n");

    // Test 1: Math question
    println!("Question 1: What is 15 * 23?");
    let response = main_agent
        .invoke_messages(vec![Message::new_human_message("What is 15 * 23?")])
        .await?;
    println!("Response: {}\n", response);

    // Test 2: Coding question
    println!("Question 2: Write a Rust function to calculate factorial");
    let response = main_agent
        .invoke_messages(vec![Message::new_human_message(
            "Write a Rust function to calculate factorial",
        )])
        .await?;
    println!("Response: {}\n", response);

    Ok(())
}
