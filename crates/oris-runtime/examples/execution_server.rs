//! Phase 2 runtime-bin execution server.
//!
//! Run with:
//!   cargo run -p oris-runtime --example execution_server --features "sqlite-persistence,execution-server"

#[cfg(all(feature = "sqlite-persistence", feature = "execution-server"))]
use std::collections::HashMap;
#[cfg(all(feature = "sqlite-persistence", feature = "execution-server"))]
use std::sync::Arc;

#[cfg(all(feature = "sqlite-persistence", feature = "execution-server"))]
use axum::http::StatusCode;
#[cfg(all(feature = "sqlite-persistence", feature = "execution-server"))]
use axum::response::IntoResponse;
#[cfg(all(feature = "sqlite-persistence", feature = "execution-server"))]
use axum::routing::get;
#[cfg(all(feature = "sqlite-persistence", feature = "execution-server"))]
use axum::{Json, Router};
#[cfg(all(feature = "sqlite-persistence", feature = "execution-server"))]
use oris_runtime::graph::{function_node, MessagesState, SqliteSaver, StateGraph, END, START};
#[cfg(all(feature = "sqlite-persistence", feature = "execution-server"))]
use oris_runtime::kernel::{build_router, ExecutionApiState};
#[cfg(all(feature = "sqlite-persistence", feature = "execution-server"))]
use oris_runtime::schemas::messages::Message;
#[cfg(all(feature = "sqlite-persistence", feature = "execution-server"))]
use serde_json::json;

#[cfg(all(feature = "sqlite-persistence", feature = "execution-server"))]
fn build_compiled(
    db_path: &str,
) -> Result<Arc<oris_runtime::graph::CompiledGraph<MessagesState>>, Box<dyn std::error::Error>> {
    let research_node = function_node("research", |_state: &MessagesState| async move {
        let mut update = HashMap::new();
        update.insert(
            "messages".to_string(),
            serde_json::to_value(vec![Message::new_ai_message(
                "Execution server step completed",
            )])?,
        );
        Ok(update)
    });

    let mut graph = StateGraph::<MessagesState>::new();
    graph.add_node("research", research_node)?;
    graph.add_edge(START, "research");
    graph.add_edge("research", END);

    let checkpointer = Arc::new(SqliteSaver::new(db_path)?);
    let compiled = graph.compile_with_persistence(Some(checkpointer), None)?;
    Ok(Arc::new(compiled))
}

#[cfg(all(feature = "sqlite-persistence", feature = "execution-server"))]
async fn healthz() -> impl IntoResponse {
    (StatusCode::OK, Json(json!({"status":"ok"})))
}

#[cfg(all(feature = "sqlite-persistence", feature = "execution-server"))]
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let db_path =
        std::env::var("ORIS_SQLITE_DB").unwrap_or_else(|_| "oris_execution_server.db".into());
    let addr = std::env::var("ORIS_SERVER_ADDR").unwrap_or_else(|_| "127.0.0.1:8080".into());
    let bearer_token = std::env::var("ORIS_API_AUTH_BEARER_TOKEN").ok();
    let api_key_id = std::env::var("ORIS_API_AUTH_API_KEY_ID").ok();
    let api_key = std::env::var("ORIS_API_AUTH_API_KEY").ok();
    let listener = tokio::net::TcpListener::bind(&addr).await?;

    let compiled = build_compiled(&db_path)?;
    let mut state = if api_key_id.is_some() && api_key.is_some() {
        ExecutionApiState::with_sqlite_idempotency(compiled, &db_path)
            .with_static_auth(bearer_token.clone(), None)
    } else {
        ExecutionApiState::with_sqlite_idempotency(compiled, &db_path)
            .with_static_auth(bearer_token.clone(), api_key.clone())
    };
    if let (Some(key_id), Some(secret)) = (api_key_id.clone(), api_key.clone()) {
        state = state.with_persisted_api_key_record(key_id, secret, true);
    }
    let app = Router::new()
        .route("/healthz", get(healthz))
        .merge(build_router(state));

    println!("execution server listening on http://{}", addr);
    if bearer_token.is_some() || api_key.is_some() || api_key_id.is_some() {
        println!("execution API auth enabled");
    }
    axum::serve(listener, app).await?;
    Ok(())
}

#[cfg(not(all(feature = "sqlite-persistence", feature = "execution-server")))]
fn main() {
    eprintln!("This example requires features: sqlite-persistence,execution-server");
    eprintln!(
        "Run: cargo run -p oris-runtime --example execution_server --features \"sqlite-persistence,execution-server\""
    );
}
