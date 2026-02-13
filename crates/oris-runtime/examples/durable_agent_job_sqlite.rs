//! Durable agent job with SQLite: interrupt, restart, resume with persistent storage.
//!
//! Same flow as `durable_agent_job`, but checkpoints are stored in a SQLite file
//! so they survive process restarts and can be used in production.
//!
//! Run with:
//!   cargo run -p oris-runtime --example durable_agent_job_sqlite --features sqlite-persistence

#[cfg(feature = "sqlite-persistence")]
use oris_runtime::graph::{
    function_node, interrupt, Command, GraphError, MessagesState, RunnableConfig, SqliteSaver,
    StateGraph, StateOrCommand, END, START,
};
#[cfg(feature = "sqlite-persistence")]
use oris_runtime::schemas::messages::Message;
#[cfg(feature = "sqlite-persistence")]
use std::collections::HashMap;

const THREAD_ID: &str = "durable-sqlite-job-1";

#[cfg(feature = "sqlite-persistence")]
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let db_path = std::env::var("ORIS_SQLITE_DB").unwrap_or_else(|_| "oris_checkpoints.db".into());

    let research_node = function_node("research", |_state: &MessagesState| async move {
        let mut update = HashMap::new();
        update.insert(
            "messages".to_string(),
            serde_json::to_value(vec![Message::new_ai_message(
                "Research done: Oris is a programmable execution runtime for AI agents in Rust.",
            )])?,
        );
        Ok(update)
    });

    let approval_node = function_node("approval", |_state: &MessagesState| async move {
        let approved = interrupt(serde_json::json!({
            "question": "Proceed with this result? (simulated: say yes to continue)",
            "context": "Durable job with SQLite — restart the process and resume with the same thread_id."
        }))
        .await
        .map_err(GraphError::InterruptError)?;

        let status = if approved.as_bool().unwrap_or(false) {
            "approved"
        } else {
            "rejected"
        };
        let mut update = HashMap::new();
        update.insert(
            "messages".to_string(),
            serde_json::to_value(vec![Message::new_ai_message(format!(
                "Approval result: {}. Job complete.",
                status
            ))])?,
        );
        Ok(update)
    });

    let mut graph = StateGraph::<MessagesState>::new();
    graph.add_node("research", research_node)?;
    graph.add_node("approval", approval_node)?;
    graph.add_edge(START, "research");
    graph.add_edge("research", "approval");
    graph.add_edge("approval", END);

    let checkpointer = std::sync::Arc::new(SqliteSaver::new(&db_path)?);
    let compiled = graph.compile_with_persistence(Some(checkpointer.clone()), None)?;
    let config = RunnableConfig::with_thread_id(THREAD_ID);

    println!(
        "Phase 1: Start job (will stop at approval). DB: {}",
        db_path
    );
    let initial_state =
        MessagesState::with_messages(vec![Message::new_human_message("Start durable job")]);
    let result = compiled
        .invoke_with_config_interrupt(StateOrCommand::State(initial_state), &config)
        .await?;

    if let Some(interrupts) = &result.interrupt {
        println!(
            "  -> Interrupted for approval ({} interrupt(s))",
            interrupts.len()
        );
        for (i, v) in interrupts.iter().enumerate() {
            println!("     [{}] {}", i + 1, v.value);
        }
        println!("\n  Exit and run again with same thread_id to resume, or continue below.");
    }

    println!("\nPhase 2: Resume (simulated approval: true)");
    let resumed = compiled
        .invoke_with_config_interrupt(
            StateOrCommand::Command(Command::resume(serde_json::json!(true))),
            &config,
        )
        .await?;

    println!("  -> Final messages:");
    for msg in &resumed.state.messages {
        println!("     {}: {}", msg.message_type.to_string(), msg.content);
    }
    println!(
        "\nDone. State persisted in {}. Restart with thread_id \"{}\" to resume from latest.",
        db_path, THREAD_ID
    );

    Ok(())
}

#[cfg(not(feature = "sqlite-persistence"))]
fn main() {
    eprintln!("This example requires the 'sqlite-persistence' feature.");
    eprintln!("Run: cargo run -p oris-runtime --example durable_agent_job_sqlite --features sqlite-persistence");
}
