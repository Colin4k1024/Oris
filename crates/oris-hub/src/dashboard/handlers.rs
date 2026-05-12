use axum::extract::{Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use std::collections::HashSet;
use std::sync::Arc;

use super::templates;
use crate::api::state::AppState;

fn html(body: String) -> Response {
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        body,
    )
        .into_response()
}

pub async fn overview(State(state): State<Arc<AppState>>) -> Response {
    let nodes = state.registry.list_active_nodes().await.unwrap_or_default();
    let subs = state.subscriptions.list(None).unwrap_or_default();

    let capabilities: Vec<String> = nodes
        .iter()
        .flat_map(|n| n.capabilities.iter().cloned())
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();

    html(templates::overview(nodes.len(), subs.len(), &capabilities))
}

pub async fn nodes(State(state): State<Arc<AppState>>) -> Response {
    let nodes = state.registry.list_active_nodes().await.unwrap_or_default();
    html(templates::nodes_page(&nodes))
}

pub async fn node_detail(
    State(state): State<Arc<AppState>>,
    Path(node_id): Path<String>,
) -> Response {
    match state.registry.get_node(&node_id).await {
        Ok(node) => html(templates::node_detail(&node)),
        Err(_) => html(templates::node_not_found(&node_id)),
    }
}

pub async fn subscriptions(State(state): State<Arc<AppState>>) -> Response {
    let subs = state.subscriptions.list(None).unwrap_or_default();
    html(templates::subscriptions_page(&subs))
}

#[derive(Debug, serde::Deserialize)]
pub struct SearchParams {
    pub q: Option<String>,
    pub task_class: Option<String>,
}

pub async fn search(Query(params): Query<SearchParams>) -> Response {
    let results_html = match &params.q {
        Some(q) if !q.is_empty() => {
            let results_content = templates::search_results(q, 0, "");
            Some(results_content)
        }
        _ => None,
    };

    html(templates::search_page(results_html.as_deref()))
}
