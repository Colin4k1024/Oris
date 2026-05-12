use axum::{
    extract::{Path, State},
    Json,
};
use std::sync::Arc;

use crate::discovery::{DiscoveryQuery, DiscoveryResult};
use crate::error::HubError;
use crate::federation::{FederatedQuery, FederatedResult};
use crate::registry::{HeartbeatRequest, HeartbeatResponse, RegisterRequest, RegisterResponse};
use crate::subscription::{CreateSubscriptionRequest, GenePromotedEvent};
use crate::validation::validate_url;

use super::state::AppState;

pub async fn register_node(
    State(state): State<Arc<AppState>>,
    Json(req): Json<RegisterRequest>,
) -> Result<Json<RegisterResponse>, HubError> {
    validate_url(&req.endpoint)?;
    let resp = state.registry.register(req).await?;
    Ok(Json(resp))
}

pub async fn heartbeat(
    State(state): State<Arc<AppState>>,
    Path(node_id): Path<String>,
    Json(mut req): Json<HeartbeatRequest>,
) -> Result<Json<HeartbeatResponse>, HubError> {
    req.node_id = node_id;
    let resp = state.registry.heartbeat(req).await?;
    Ok(Json(resp))
}

pub async fn deregister_node(
    State(state): State<Arc<AppState>>,
    Path(node_id): Path<String>,
) -> Result<Json<serde_json::Value>, HubError> {
    state.registry.deregister(&node_id).await?;
    Ok(Json(serde_json::json!({ "deleted": true })))
}

pub async fn discover_nodes(
    State(state): State<Arc<AppState>>,
    Json(query): Json<DiscoveryQuery>,
) -> Result<Json<DiscoveryResult>, HubError> {
    let result = state.discovery.discover(query).await?;
    Ok(Json(result))
}

pub async fn get_node(
    State(state): State<Arc<AppState>>,
    Path(node_id): Path<String>,
) -> Result<Json<crate::registry::NodeInfo>, HubError> {
    let node = state.registry.get_node(&node_id).await?;
    Ok(Json(node))
}

pub async fn federated_search(
    State(state): State<Arc<AppState>>,
    Json(query): Json<FederatedQuery>,
) -> Result<Json<FederatedResult>, HubError> {
    let result = state.federation.search(query).await?;
    Ok(Json(result))
}

pub async fn get_stats(
    State(state): State<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, HubError> {
    let nodes = state.registry.list_active_nodes().await?;
    Ok(Json(serde_json::json!({
        "active_nodes": nodes.len(),
        "total_capabilities": nodes.iter().flat_map(|n| n.capabilities.iter()).collect::<std::collections::HashSet<_>>().len(),
    })))
}

pub async fn create_subscription(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateSubscriptionRequest>,
) -> Result<Json<crate::subscription::Subscription>, HubError> {
    validate_url(&req.callback_url)?;
    let sub = state.subscriptions.create(&req)?;
    Ok(Json(sub))
}

pub async fn list_subscriptions(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<crate::subscription::Subscription>>, HubError> {
    let subs = state.subscriptions.list(None)?;
    Ok(Json(subs))
}

pub async fn delete_subscription(
    State(state): State<Arc<AppState>>,
    Path(sub_id): Path<String>,
) -> Result<Json<serde_json::Value>, HubError> {
    state.subscriptions.delete(&sub_id)?;
    Ok(Json(serde_json::json!({ "deleted": true })))
}

pub async fn gene_promoted(
    State(state): State<Arc<AppState>>,
    Json(event): Json<GenePromotedEvent>,
) -> Result<Json<crate::subscription::manager::NotifyResult>, HubError> {
    let result = state.subscriptions.notify_gene_promoted(event).await?;
    Ok(Json(result))
}
