//! Deep Agent file system tools: ls, read_file, write_file, edit_file.
//!
//! Demonstrates a Deep Agent with a workspace root so the agent can:
//! - **ls**: list directory contents (name, type, size, modified)
//! - **read_file**: read file with optional offset/limit (line numbers)
//! - **write_file**: create or overwrite files (creates parents if needed)
//! - **edit_file**: exact string replacements in a file
//!
//! Run with:
//! ```bash
//! cargo run --example deep_agent_filesystem
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

    let workspace = std::env::temp_dir().join("oris_deep_agent_fs_example");
    std::fs::create_dir_all(&workspace)?;
    std::fs::write(workspace.join("hello.txt"), "Hello, world!\nSecond line.\n")?;

    let config = DeepAgentConfig::new()
        .with_planning(false)
        .with_filesystem(true)
        .with_workspace_root(workspace.clone());

    let agent = create_deep_agent(
        "gpt-4o-mini",
        &[],
        Some(
            "You have access to file system tools in a workspace: ls, read_file, write_file, edit_file. \
             Paths are relative to the workspace. Use them to answer the user.",
        ),
        config,
    )?;

    println!("=== Deep Agent file system tools ===\n");
    println!("Workspace: {}\n", workspace.display());

    // List and read
    let result = agent
        .invoke(prompt_args! {
            "messages" => vec![
                Message::new_human_message("List files in the workspace and read the contents of hello.txt.")
            ]
        })
        .await?;
    println!("Response: {}\n", result);

    // Write new file
    let result2 = agent
        .invoke_messages(vec![Message::new_human_message(
            "Create a file named summary.txt with the single line: Summary of workspace.",
        )])
        .await?;
    println!("Response 2: {}\n", result2);

    // Edit existing file
    let result3 = agent
        .invoke_messages(vec![Message::new_human_message(
            "In hello.txt, replace 'world' with 'Deep Agent'.",
        )])
        .await?;
    println!("Response 3: {}\n", result3);

    Ok(())
}
