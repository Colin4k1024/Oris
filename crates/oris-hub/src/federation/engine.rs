use chrono::Utc;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::timeout;
use tracing::{info, warn};

use super::types::*;
use crate::error::HubError;
use crate::registry::{NodeInfo, RegistryService};

const DEFAULT_TIMEOUT_MS: u64 = 500;
const DEFAULT_RESULT_LIMIT: usize = 50;

pub struct FederationEngine {
    registry: Arc<RegistryService>,
    http_client: reqwest::Client,
}

impl FederationEngine {
    pub fn new(registry: Arc<RegistryService>) -> Self {
        let http_client = reqwest::Client::builder()
            .timeout(Duration::from_millis(DEFAULT_TIMEOUT_MS * 2))
            .build()
            .expect("failed to build HTTP client");

        Self {
            registry,
            http_client,
        }
    }

    pub async fn search(&self, query: FederatedQuery) -> Result<FederatedResult, HubError> {
        let nodes = self.resolve_target_nodes(&query).await?;
        let nodes_queried = nodes.len();
        let timeout_ms = query.timeout_ms.unwrap_or(DEFAULT_TIMEOUT_MS);
        let limit = query.limit.unwrap_or(DEFAULT_RESULT_LIMIT);

        let mut handles = Vec::with_capacity(nodes.len());
        for node in &nodes {
            let client = self.http_client.clone();
            let endpoint = format!("{}/experience/search", node.endpoint.trim_end_matches('/'));
            let body = serde_json::json!({
                "query": query.query,
                "task_class": query.task_class,
                "min_confidence": query.min_confidence,
                "limit": limit,
            });
            let node_id = node.node_id.clone();
            let timeout_dur = Duration::from_millis(timeout_ms);

            handles.push(tokio::spawn(async move {
                let result = timeout(timeout_dur, async {
                    client
                        .post(&endpoint)
                        .json(&body)
                        .send()
                        .await
                        .map_err(|e| e.to_string())?
                        .json::<NodeSearchResponse>()
                        .await
                        .map_err(|e| e.to_string())
                })
                .await;

                (node_id, result)
            }));
        }

        let mut all_results: Vec<GeneResult> = Vec::new();
        let mut timeout_nodes: Vec<String> = Vec::new();
        let mut responded = 0usize;

        for handle in handles {
            match handle.await {
                Ok((_node_id, Ok(Ok(response)))) => {
                    responded += 1;
                    all_results.extend(response.genes);
                }
                Ok((node_id, Ok(Err(e)))) => {
                    warn!(node_id = %node_id, error = %e, "node search failed");
                    timeout_nodes.push(node_id);
                }
                Ok((node_id, Err(_))) => {
                    warn!(node_id = %node_id, "node search timed out");
                    timeout_nodes.push(node_id);
                }
                Err(e) => {
                    warn!(error = %e, "task join error");
                }
            }
        }

        all_results.sort_by(|a, b| {
            b.confidence
                .partial_cmp(&a.confidence)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        all_results.dedup_by(|a, b| a.gene_id == b.gene_id);
        all_results.truncate(limit);

        let coverage = if nodes_queried > 0 {
            responded as f64 / nodes_queried as f64
        } else {
            0.0
        };

        info!(
            nodes_queried = nodes_queried,
            responded = responded,
            results = all_results.len(),
            "federation search complete"
        );

        Ok(FederatedResult {
            results: all_results,
            meta: FederationMeta {
                nodes_queried,
                nodes_responded: responded,
                coverage,
                freshness: Utc::now(),
                timeout_nodes,
            },
        })
    }

    async fn resolve_target_nodes(
        &self,
        query: &FederatedQuery,
    ) -> Result<Vec<NodeInfo>, HubError> {
        let active = self.registry.list_active_nodes().await?;

        if let Some(ref targets) = query.target_nodes {
            Ok(active
                .into_iter()
                .filter(|n| targets.contains(&n.node_id))
                .collect())
        } else {
            Ok(active)
        }
    }
}
