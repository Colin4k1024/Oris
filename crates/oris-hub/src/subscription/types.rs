use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Subscription {
    pub id: String,
    pub subscriber_node_id: String,
    pub callback_url: String,
    pub filter: SubscriptionFilter,
    pub created_at: DateTime<Utc>,
    pub active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionFilter {
    pub task_class: Option<String>,
    pub min_confidence: Option<f64>,
    pub source_nodes: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateSubscriptionRequest {
    pub subscriber_node_id: String,
    pub callback_url: String,
    pub filter: SubscriptionFilter,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenePromotedEvent {
    pub gene_id: String,
    pub gene_name: String,
    pub task_class: String,
    pub confidence: f64,
    pub source_node_id: String,
    pub promoted_at: DateTime<Utc>,
}
