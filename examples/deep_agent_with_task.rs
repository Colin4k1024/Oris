//! Deep Agent with task tool: delegate work to specialized subagents.
//!
//! Demonstrates [create_deep_agent] with the **task** tool:
//! - Main agent can call the single `task` tool with a `subagent_id` and `input`
//! - The task tool routes to the appropriate subagent (e.g. researcher, coder)
//!
//! Run with:
//! ```bash
//! cargo run --example deep_agent_with_task
//! ```

use std::sync::Arc;

use oris::{
    agent::{create_agent, create_deep_agent, DeepAgentConfig},
    schemas::messages::Message,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();

    // Create specialized subagents
    let researcher = Arc::new(create_agent(
        "gpt-4o-mini",
        &[],
        Some(
            "You are a research assistant. Answer questions concisely with facts and sources when relevant.",
        ),
        None,
    )?);

    let coder = Arc::new(create_agent(
        "gpt-4o-mini",
        &[],
        Some("You are a coding assistant. Provide short, clear code snippets and explanations."),
        None,
    )?);

    let config = DeepAgentConfig::new()
        .with_planning(true)
        .with_filesystem(false) // no workspace in this example
        .with_subagent(
            Arc::clone(&researcher),
            "researcher",
            "Use for factual questions, research, and summarization",
        )
        .with_subagent(
            Arc::clone(&coder),
            "coder",
            "Use for programming, code examples, and technical how-tos",
        );

    let main_agent = create_deep_agent(
        "gpt-4o-mini",
        &[],
        Some(
            "You are a coordinator. You have a 'task' tool that delegates to specialized subagents: \
             researcher (for facts and research) and coder (for code and tech). \
             Use the task tool with the right subagent_id and input when the user asks for research or code.",
        ),
        config,
    )?;

    println!("=== Deep Agent with task tool (subagents) ===\n");

    // Question that benefits from the researcher subagent
    println!("Question: What is the capital of France?");
    let response = main_agent
        .invoke_messages(vec![Message::new_human_message(
            "What is the capital of France?",
        )])
        .await?;
    println!("Response: {}\n", response);

    // Question that benefits from the coder subagent
    println!("Question: How do I reverse a string in Python?");
    let response2 = main_agent
        .invoke_messages(vec![Message::new_human_message(
            "How do I reverse a string in Python? Give a one-line example.",
        )])
        .await?;
    println!("Response: {}\n", response2);

    Ok(())
}
