//! Minimal CLI for durable job: run, list, inspect, resume, replay, cancel.
//!
//! Demonstrates Phase 2 operator API with local SQLite persistence.
//!
//! Run with:
//!   cargo run -p oris-runtime --example cli_durable_job --features sqlite-persistence -- run --thread-id my-job
//!   cargo run -p oris-runtime --example cli_durable_job --features sqlite-persistence -- list --thread-id my-job
//!   cargo run -p oris-runtime --example cli_durable_job --features sqlite-persistence -- inspect --thread-id my-job
//!   cargo run -p oris-runtime --example cli_durable_job --features sqlite-persistence -- resume --thread-id my-job
//!   cargo run -p oris-runtime --example cli_durable_job --features sqlite-persistence -- replay --thread-id my-job
//!   cargo run -p oris-runtime --example cli_durable_job --features sqlite-persistence -- cancel --thread-id my-job
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
        if args[i] == "run"
            || args[i] == "list"
            || args[i] == "inspect"
            || args[i] == "resume"
            || args[i] == "replay"
            || args[i] == "cancel"
        {
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
async fn execute_command(
    compiled: &oris_runtime::graph::CompiledGraph<MessagesState>,
    cmd: &str,
    thread_id: &str,
    config: &RunnableConfig,
) -> Result<String, Box<dyn std::error::Error>> {
    match cmd {
        "run" => {
            let initial = MessagesState::with_messages(vec![Message::new_human_message("CLI run")]);
            let state = compiled.invoke_with_config(Some(initial), config).await?;
            Ok(format!("Run completed. Messages: {}", state.messages.len()))
        }
        "list" => {
            let history = compiled.get_state_history(config).await?;
            let mut output = vec![format!(
                "Checkpoints for thread_id '{}': {}",
                thread_id,
                history.len()
            )];
            for (i, snap) in history.iter().enumerate() {
                output.push(format!(
                    "  {}  checkpoint_id={:?}  created_at={}",
                    i + 1,
                    snap.checkpoint_id(),
                    snap.created_at
                ));
            }
            Ok(output.join("\n"))
        }
        "inspect" => {
            let snapshot = compiled.get_state(config).await?;
            Ok(format!(
                "Inspect thread '{}' checkpoint={:?} messages={}",
                thread_id,
                snapshot.checkpoint_id(),
                snapshot.values.messages.len()
            ))
        }
        "resume" => {
            let state = compiled.invoke_with_config(None, config).await?;
            Ok(format!(
                "Resume completed. Messages: {}",
                state.messages.len()
            ))
        }
        "replay" => {
            let state = compiled.invoke_with_config(None, config).await?;
            Ok(format!(
                "Replay completed for thread '{}'. Messages: {}",
                thread_id,
                state.messages.len()
            ))
        }
        "cancel" => Ok(format!(
            "Cancel accepted for thread '{}'. (local CLI stub: no active worker to signal)",
            thread_id
        )),
        _ => Err(format!("Unknown command: {}", cmd).into()),
    }
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
            eprintln!("  inspect --thread-id <id>   Inspect latest checkpoint");
            eprintln!("  resume --thread-id <id> [--checkpoint-id <id>]  Resume from latest or checkpoint");
            eprintln!("  replay --thread-id <id> [--checkpoint-id <id>]  Replay from latest or checkpoint");
            eprintln!("  cancel --thread-id <id>    Mark local cancel request (stub)");
            std::process::exit(1);
        }
    };

    let (compiled, _checkpointer) = build_graph_and_compiled(&db_path)?;
    let config = if let Some(cp) = checkpoint_id {
        RunnableConfig::with_checkpoint(&thread_id, &cp)
    } else {
        RunnableConfig::with_thread_id(&thread_id)
    };

    let output = execute_command(&compiled, &cmd, &thread_id, &config).await?;
    println!("{}", output);

    Ok(())
}

#[cfg(not(feature = "sqlite-persistence"))]
fn main() {
    eprintln!("This example requires the 'sqlite-persistence' feature.");
    eprintln!("Run: cargo run -p oris-runtime --example cli_durable_job --features sqlite-persistence -- run --thread-id my-job");
}

#[cfg(all(test, feature = "sqlite-persistence"))]
mod tests {
    use super::*;

    #[test]
    fn parse_args_supports_phase2_commands() {
        let args = vec![
            "inspect".to_string(),
            "--thread-id".to_string(),
            "job-a".to_string(),
        ];
        let parsed = parse_args(&args).expect("inspect should parse");
        assert_eq!(parsed.0, "inspect");
        assert_eq!(parsed.1, "job-a");

        let args = vec![
            "replay".to_string(),
            "--thread-id".to_string(),
            "job-a".to_string(),
            "--checkpoint-id".to_string(),
            "cp-1".to_string(),
        ];
        let parsed = parse_args(&args).expect("replay should parse");
        assert_eq!(parsed.0, "replay");
        assert_eq!(parsed.2.as_deref(), Some("cp-1"));

        let args = vec![
            "cancel".to_string(),
            "--thread-id".to_string(),
            "job-a".to_string(),
        ];
        let parsed = parse_args(&args).expect("cancel should parse");
        assert_eq!(parsed.0, "cancel");
    }

    #[test]
    fn execute_command_handles_phase2_dispatch_paths() {
        let (compiled, _checkpointer) = build_graph_and_compiled(":memory:").expect("build graph");
        let config = RunnableConfig::with_thread_id("dispatch-test");
        let rt = tokio::runtime::Runtime::new().expect("runtime");
        rt.block_on(async {
            let cancel_output = execute_command(&compiled, "cancel", "dispatch-test", &config)
                .await
                .expect("cancel output");
            assert!(cancel_output.contains("Cancel accepted"));

            let err = execute_command(&compiled, "unknown", "dispatch-test", &config)
                .await
                .expect_err("unknown should fail");
            assert!(err.to_string().contains("Unknown command"));
        });
    }
}
