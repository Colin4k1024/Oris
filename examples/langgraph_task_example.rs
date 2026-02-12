use oris::langgraph::{
    execute_task_with_cache, function_node, FunctionTask, InMemorySaver, LangGraphError,
    MessagesState, RunnableConfig, StateGraph, Task, TaskCache, END, START,
};
use oris::schemas::messages::Message;
use serde_json::Value;
use std::sync::Arc;

/// Task example for LangGraph
///
/// This example demonstrates:
/// 1. Creating tasks to wrap non-deterministic operations
/// 2. Using task cache to avoid re-execution
/// 3. Tasks in nodes
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create a task that makes an API call (simulated)
    let api_task: Arc<dyn Task> = Arc::new(FunctionTask::new("api_call", |input: Value| {
        Box::pin(async move {
            // Simulate API call
            let url = input
                .get("url")
                .and_then(|v| v.as_str())
                .unwrap_or("https://example.com");

            // In a real scenario, this would be an actual HTTP request
            // For demo, we'll just return a mock response
            Ok(serde_json::json!({
                "url": url,
                "response": format!("Response from {}", url),
                "timestamp": std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs()
            }))
        })
    }));

    // Create a node that uses the task
    let api_task_clone = api_task.clone();
    let api_node = function_node("api_node", move |_state: &MessagesState| {
        let api_task = api_task_clone.clone();
        async move {
            use std::collections::HashMap;

            // Create task cache
            let cache = TaskCache::new();

            // Execute task with cache
            let task_input = serde_json::json!({
                "url": "https://api.example.com/data"
            });

            let task_result = execute_task_with_cache(api_task.as_ref(), task_input, Some(&cache))
                .await
                .map_err(|e| LangGraphError::ExecutionError(e.to_string()))?;

            // Use task result in state update
            let mut update = HashMap::new();
            update.insert(
                "messages".to_string(),
                serde_json::to_value(vec![Message::new_ai_message(format!(
                    "API response: {}",
                    task_result
                        .get("response")
                        .unwrap_or(&serde_json::json!("No response"))
                ))])?,
            );

            Ok(update)
        }
    });

    // Build the graph
    let mut graph = StateGraph::<MessagesState>::new();
    graph.add_node("api_node", api_node)?;
    graph.add_edge(START, "api_node");
    graph.add_edge("api_node", END);

    // Create checkpointer
    let checkpointer = std::sync::Arc::new(InMemorySaver::new());

    // Compile with checkpointer
    let compiled = graph.compile_with_persistence(Some(checkpointer.clone()), None)?;

    // Execute the graph
    let config = RunnableConfig::with_thread_id("thread-task-1");
    let initial_state =
        MessagesState::with_messages(vec![Message::new_human_message("Fetch data")]);
    let final_state = compiled
        .invoke_with_config(Some(initial_state), &config)
        .await?;

    println!("Final messages:");
    for message in &final_state.messages {
        println!(
            "  {}: {}",
            message.message_type.to_string(),
            message.content
        );
    }

    // If we resume from checkpoint, the task result should be cached
    // and not re-executed
    println!("\nNote: If resuming from checkpoint, task results are cached");
    println!("and the task will not be re-executed.");

    Ok(())
}
