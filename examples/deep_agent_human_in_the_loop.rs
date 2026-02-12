//! Deep Agent human-in-the-loop (HILP) example.
//!
//! Aligned with [Human-in-the-loop](https://docs.langchain.com/oss/python/deepagents/human-in-the-loop).
//! Demonstrates:
//! - `interrupt_on` config (per-tool; allowed_decisions: approve, edit, reject)
//! - Checkpointer (required for HILP; same thread_id when resuming)
//! - Invoke → interrupt with action_requests/review_configs → resume with decisions
//!
//! Run with:
//! ```bash
//! cargo run --example deep_agent_human_in_the_loop
//! ```

use std::collections::HashMap;
use std::sync::Arc;

use oris::agent::{
    create_deep_agent, AgentInput, AgentInvokeResult, DeepAgentConfig, HitlDecision,
    InMemoryAgentSaver, InterruptConfig, InterruptPayload,
};
use oris::langgraph::RunnableConfig;
use oris::schemas::messages::Message;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();

    let workspace = std::env::temp_dir().join("oris_deep_agent_hilp_example");
    std::fs::create_dir_all(&workspace)?;
    std::fs::write(workspace.join("readme.txt"), "Example file for HILP.\n")?;

    let mut interrupt_on = HashMap::new();
    interrupt_on.insert(
        "write_file".to_string(),
        InterruptConfig::enabled().with_allowed_decisions(vec![
            "approve".to_string(),
            "edit".to_string(),
            "reject".to_string(),
        ]),
    );
    interrupt_on.insert(
        "edit_file".to_string(),
        InterruptConfig::enabled()
            .with_allowed_decisions(vec!["approve".to_string(), "reject".to_string()]),
    );

    let checkpointer: Arc<dyn oris::agent::AgentCheckpointer> =
        Arc::new(InMemoryAgentSaver::new());
    let config = DeepAgentConfig::new()
        .with_planning(false)
        .with_filesystem(true)
        .with_workspace_root(workspace)
        .with_interrupt_on(interrupt_on)
        .with_checkpointer(Some(Arc::clone(&checkpointer)));

    let agent = create_deep_agent(
        "gpt-4o-mini",
        &[],
        Some(
            "You are a deep agent. Use ls, read_file, write_file, edit_file. \
             When the user asks to write or edit a file, do it.",
        ),
        config,
    )?;

    let thread_id = "hilp-thread-1";
    let run_config = RunnableConfig::with_thread_id(thread_id);

    println!("=== Deep Agent Human-in-the-Loop ===\n");
    println!("Request: write a new file and edit readme.txt\n");

    let prompt_args = oris::prompt_args! {
        "messages" => vec![
            Message::new_human_message(
                "Write a file called hello.txt with content 'Hello HILP' and add a line to readme.txt saying 'Edited by HILP'."
            )
        ]
    };

    let result = agent
        .invoke_with_config(AgentInput::State(prompt_args), &run_config)
        .await?;

    match result {
        AgentInvokeResult::Complete(output) => {
            println!("Completed without interrupt:\n{}\n", output);
        }
        AgentInvokeResult::Interrupt { interrupt_value } => {
            println!("Interrupted for human approval.\n");
            let payload: InterruptPayload = serde_json::from_value(interrupt_value.clone())
                .unwrap_or_else(|_| InterruptPayload {
                    action_requests: vec![],
                    review_configs: vec![],
                });
            for (i, action) in payload.action_requests.iter().enumerate() {
                let review = payload.review_configs.get(i);
                println!(
                    "  Tool: {} | args: {} | allowed: {:?}",
                    action.name,
                    action.args,
                    review.map(|r| &r.allowed_decisions)
                );
            }
            println!("\nResuming with approve for all...\n");
            let decisions: Vec<HitlDecision> = payload
                .action_requests
                .iter()
                .map(|_| HitlDecision::Approve)
                .collect();
            let resume_value = serde_json::json!({ "decisions": decisions });
            let resume_result = agent
                .invoke_with_config(AgentInput::Resume(resume_value), &run_config)
                .await?;
            match resume_result {
                AgentInvokeResult::Complete(output) => {
                    println!("Final output:\n{}\n", output);
                }
                AgentInvokeResult::Interrupt { .. } => {
                    println!("Interrupted again (unexpected in this example).");
                }
            }
        }
    }

    Ok(())
}
