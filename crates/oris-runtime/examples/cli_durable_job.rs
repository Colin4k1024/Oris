//! Minimal CLI for durable job: run, list checkpoints, resume.
//!
//! Demonstrates Phase 1 operator API: start a run, list checkpoints, resume by thread_id (and optional checkpoint_id).
//!
//! Run with:
//!   cargo run -p oris-runtime --example cli_durable_job --features sqlite-persistence -- run --thread-id my-job
//!   cargo run -p oris-runtime --example cli_durable_job --features sqlite-persistence -- list --thread-id my-job
//!   cargo run -p oris-runtime --example cli_durable_job --features sqlite-persistence -- resume --thread-id my-job
//!   cargo run -p oris-runtime --example cli_durable_job --features sqlite-persistence -- resume --thread-id my-job --checkpoint-id <id>

#[cfg(feature = "sqlite-persistence")]
use oris_runtime::graph::{
    function_node, MessagesState, RunnableConfig, SqliteSaver, StateGraph, END, START,
};
#[cfg(feature = "sqlite-persistence")]
use oris_runtime::schemas::messages::Message;
#[cfg(feature = "sqlite-persistence")]
use std::collections::HashMap;

#[cfg(feature = "sqlite-persistence")]
fn parse_args(args: &[String]) -> Option<(String, String, Option<String>)> {
    // subcommand --thread-id <id> [--checkpoint-id <id>]
    let mut i = 0;
    let mut cmd = None;
    let mut thread_id = None;
    let mut checkpoint_id = None;
    while i < args.len() {
        if args[i] == "run" || args[i] == "list" || args[i] == "resume" {
            cmd = Some(args[i].clone());
            i += 1;
            continue;
        }
        if args[i] == "--thread-id" && i + 1 < args.len() {
            thread_id = Some(args[i + 1].clone());
            i += 2;
            continue;
        }
        if args[i] == "--checkpoint-id" && i + 1 < args.len() {
            checkpoint_id = Some(args[i + 1].clone());
            i += 2;
            continue;
        }
        i += 1;
    }
    let cmd = cmd?;
    let thread_id = thread_id?;
    Some((cmd, thread_id, checkpoint_id))
}

#[cfg(feature = "sqlite-persistence")]
fn build_graph_and_compiled(
    db_path: &str,
) -> Result<
    (
        oris_runtime::graph::CompiledGraph<MessagesState>,
        std::sync::Arc<SqliteSaver<MessagesState>>,
    ),
    Box<dyn std::error::Error>,
> {
    let research_node = function_node("research", |_state: &MessagesState| async move {
        let mut update = HashMap::new();
        update.insert(
            "messages".to_string(),
            serde_json::to_value(vec![Message::new_ai_message(
                "Research done: Oris durable execution in Rust.",
            )])?,
        );
        Ok(update)
    });

    let approval_node = function_node("approval", |_state: &MessagesState| async move {
        let mut update = HashMap::new();
        update.insert(
            "messages".to_string(),
            serde_json::to_value(vec![Message::new_ai_message(
                "Approval step (no interrupt in run)",
            )])?,
        );
        Ok(update)
    });

    let mut graph = StateGraph::<MessagesState>::new();
    graph.add_node("research", research_node)?;
    graph.add_node("approval", approval_node)?;
    graph.add_edge(START, "research");
    graph.add_edge("research", "approval");
    graph.add_edge("approval", END);

    let checkpointer = std::sync::Arc::new(SqliteSaver::new(db_path)?);
    let compiled = graph.compile_with_persistence(Some(checkpointer.clone()), None)?;
    Ok((compiled, checkpointer))
}

#[cfg(feature = "sqlite-persistence")]
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let db_path =
        std::env::var("ORIS_SQLITE_DB").unwrap_or_else(|_| "oris_cli_checkpoints.db".into());

    let (cmd, thread_id, checkpoint_id) = match parse_args(&args) {
        Some(t) => t,
        None => {
            eprintln!("Usage:");
            eprintln!("  run   --thread-id <id>     Start a run");
            eprintln!("  list  --thread-id <id>     List checkpoints");
            eprintln!("  resume --thread-id <id> [--checkpoint-id <id>]  Resume from latest or checkpoint");
            std::process::exit(1);
        }
    };

    let (compiled, _checkpointer) = build_graph_and_compiled(&db_path)?;
    let config = if let Some(cp) = checkpoint_id {
        RunnableConfig::with_checkpoint(&thread_id, &cp)
    } else {
        RunnableConfig::with_thread_id(&thread_id)
    };

    match cmd.as_str() {
        "run" => {
            let initial = MessagesState::with_messages(vec![Message::new_human_message("CLI run")]);
            let state = compiled.invoke_with_config(Some(initial), &config).await?;
            println!("Run completed. Messages: {}", state.messages.len());
        }
        "list" => {
            let history = compiled.get_state_history(&config).await?;
            println!(
                "Checkpoints for thread_id '{}': {}",
                thread_id,
                history.len()
            );
            for (i, snap) in history.iter().enumerate() {
                println!(
                    "  {}  checkpoint_id={:?}  created_at={}",
                    i + 1,
                    snap.checkpoint_id(),
                    snap.created_at
                );
            }
        }
        "resume" => {
            let state = compiled.invoke_with_config(None, &config).await?;
            println!("Resume completed. Messages: {}", state.messages.len());
        }
        _ => {
            eprintln!("Unknown command: {}", cmd);
            std::process::exit(1);
        }
    }

    Ok(())
}

#[cfg(not(feature = "sqlite-persistence"))]
fn main() {
    eprintln!("This example requires the 'sqlite-persistence' feature.");
    eprintln!("Run: cargo run -p oris-runtime --example cli_durable_job --features sqlite-persistence -- run --thread-id my-job");
}
