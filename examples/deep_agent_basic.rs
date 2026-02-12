//! Deep Agent basic example: planning (write_todos) and file system tools.
//!
//! Demonstrates [create_deep_agent] with default config:
//! - **Planning**: `write_todos` tool for task decomposition and progress tracking
//! - **Context management**: `ls`, `read_file`, `write_file`, `edit_file` in a workspace
//! - **Long-term memory**: InMemoryStore for todos (and optional custom store)
//!
//! Run with:
//! ```bash
//! cargo run --example deep_agent_basic
//! ```

use oris::{
    agent::{create_deep_agent, DeepAgentConfig},
    chain::Chain,
    prompt_args,
    schemas::messages::Message,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();

    // Use a temp directory as workspace so file tools have somewhere to operate
    let workspace = std::env::temp_dir().join("oris_deep_agent_example");
    std::fs::create_dir_all(&workspace)?;
    // Create a sample file for read/edit demos
    std::fs::write(
        workspace.join("notes.txt"),
        "Line 1: Hello\nLine 2: World\n",
    )?;

    let config = DeepAgentConfig::new()
        .with_planning(true)
        .with_filesystem(true)
        .with_workspace_root(workspace.clone());

    let agent = create_deep_agent(
        "gpt-4o-mini",
        &[], // no extra tools; built-in write_todos + file tools are added
        Some(
            "You are a deep agent with planning and file system tools. \
             Use write_todos to break complex tasks into steps. \
             Use ls, read_file, write_file, edit_file to work inside the workspace.",
        ),
        config,
    )?;

    println!("=== Deep Agent (planning + filesystem) ===\n");
    println!("Workspace: {}", workspace.display());

    // Invoke with a request that may use write_todos and file tools
    let result = agent
        .invoke(prompt_args! {
            "messages" => vec![
                Message::new_human_message(
                    "List the files in the workspace, then read notes.txt and tell me the first line."
                )
            ]
        })
        .await?;

    println!("Response: {}\n", result);

    // Second turn: can involve editing or planning
    let result2 = agent
        .invoke_messages(vec![Message::new_human_message(
            "Add a third line to notes.txt saying 'Line 3: From Deep Agent'.",
        )])
        .await?;

    println!("Response 2: {}\n", result2);

    Ok(())
}
