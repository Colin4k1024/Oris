//! Deep Agent customization: context, middleware, skills, memory, middleware-style prompts, and optional custom LLM.
//!
//! Aligned with [Customize Deep Agents](https://docs.langchain.com/oss/python/deepagents/customization) and
//! [Deep Agents Middleware](https://docs.langchain.com/oss/python/deepagents/middleware).
//! Demonstrates:
//! - **Context**: custom `ToolContext` (e.g. session_id, workspace_root)
//! - **Middleware**: e.g. LoggingMiddleware
//! - **Planning/Filesystem prompts**: `with_planning_system_prompt`, `with_filesystem_system_prompt` (TodoListMiddleware / FilesystemMiddleware-style)
//! - **Custom tool descriptions**: `with_custom_tool_description` for built-in tools
//! - **Skills**: inline skill content appended to system prompt under "## Skills"
//! - **Memory**: inline memory content under "## Memory"
//! - **create_deep_agent_from_llm**: use a custom LLM instance instead of model string
//!
//! Run with:
//! ```bash
//! cargo run --example deep_agent_customization
//! ```

use std::sync::Arc;

use oris::{
    agent::{
        create_deep_agent, create_deep_agent_from_llm, detect_and_create_llm, DeepAgentConfig,
        LoggingMiddleware,
    },
    chain::Chain,
    prompt_args,
    schemas::messages::Message,
    tools::SimpleContext,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();

    // Custom context: session and workspace for tools that need them
    let ctx = Arc::new(
        SimpleContext::new()
            .with_session_id("custom-session-123".to_string())
            .with_custom(
                "workspace_root".to_string(),
                std::env::temp_dir().display().to_string(),
            ),
    );

    // Inline skill: instructions the agent can use
    let skill_content =
        r#"When the user asks for a summary, always respond in three bullet points."#;

    // Inline memory: persistent context (e.g. from AGENTS.md)
    let memory_content = "Project convention: prefer short, actionable responses.";

    let config = DeepAgentConfig::new()
        .with_planning(true)
        .with_filesystem(false)
        .with_context(ctx)
        .with_middleware(vec![Arc::new(LoggingMiddleware::new())])
        .with_planning_system_prompt(Some(
            "Use the write_todos tool to break work into steps before answering.".to_string(),
        ))
        .with_custom_tool_description(
            "write_todos",
            "Update the to-do list for this session. Call when the user mentions tasks or a plan.",
        )
        .with_skill_content("summary_rules", skill_content)
        .with_memory_content("conventions", memory_content);

    let agent = create_deep_agent(
        "gpt-4o-mini",
        &[],
        Some("You are a helpful deep agent. Follow the Skills and Memory sections."),
        config,
    )?;

    println!("=== Deep Agent customization ===\n");

    let result = agent
        .invoke(prompt_args! {
            "messages" => vec![
                Message::new_human_message("Summarize what you know about project conventions in one sentence.")
            ]
        })
        .await?;

    println!("Response: {}\n", result);

    // Optional: create from custom LLM instead of model string
    let llm = detect_and_create_llm("gpt-4o-mini")?;
    let config_llm = DeepAgentConfig::new()
        .with_planning(true)
        .with_filesystem(false)
        .with_skill_content("tip", "Always be concise.");
    let agent_from_llm = create_deep_agent_from_llm(llm, &[], None, config_llm)?;
    let result2 = agent_from_llm
        .invoke_messages(vec![Message::new_human_message("Say hello in one word.")])
        .await?;
    println!("From-LLM response: {}", result2);

    Ok(())
}
