use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use axum::routing::get;
use axum::{Json, Router};
use oris_runtime::graph::{function_node, MessagesState, SqliteSaver, StateGraph, END, START};
use oris_runtime::kernel::{build_router, ExecutionApiState};
use oris_runtime::schemas::messages::Message;
use serde_json::json;
use tracing_subscriber::EnvFilter;

fn build_compiled(db_path: &str) -> Result<Arc<oris_runtime::graph::CompiledGraph<MessagesState>>> {
    let research = function_node("research", |_state: &MessagesState| async move {
        let mut update = HashMap::new();
        update.insert(
            "messages".to_string(),
            serde_json::to_value(vec![Message::new_ai_message(
                "starter workflow step completed",
            )])?,
        );
        Ok(update)
    });

    let mut graph = StateGraph::<MessagesState>::new();
    graph.add_node("research", research)?;
    graph.add_edge(START, "research");
    graph.add_edge("research", END);

    let checkpointer = Arc::new(SqliteSaver::new(db_path)?);
    let compiled = graph.compile_with_persistence(Some(checkpointer), None)?;
    Ok(Arc::new(compiled))
}

async fn healthz() -> Json<serde_json::Value> {
    Json(json!({ "status": "ok" }))
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                EnvFilter::new("info,oris_runtime=info,oris_starter_axum=info")
            }),
        )
        .init();

    let db_path = std::env::var("ORIS_SQLITE_DB").unwrap_or_else(|_| "oris_starter.db".into());
    let addr = std::env::var("ORIS_SERVER_ADDR").unwrap_or_else(|_| "127.0.0.1:8080".into());
    let bearer_token = std::env::var("ORIS_API_AUTH_BEARER_TOKEN").ok();
    let api_key = std::env::var("ORIS_API_AUTH_API_KEY").ok();

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    let compiled = build_compiled(&db_path)?;
    let state = ExecutionApiState::with_sqlite_idempotency(compiled, &db_path)
        .with_static_auth(bearer_token.clone(), api_key.clone());

    let app = Router::new()
        .route("/healthz", get(healthz))
        .merge(build_router(state));

    tracing::info!("oris starter server listening on http://{}", addr);
    if bearer_token.is_some() || api_key.is_some() {
        tracing::info!("execution API auth enabled");
    }
    axum::serve(listener, app).await?;
    Ok(())
}
