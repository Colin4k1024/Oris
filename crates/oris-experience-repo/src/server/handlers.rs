//! HTTP handlers for Experience Repository.

use std::collections::HashMap;
use std::sync::Arc;

use axum::{
    extract::{Query, State},
    routing::get,
    Json, Router,
};
use oris_genestore::{GeneQuery, GeneStore};
use tokio::sync::Mutex;

use crate::api::request::FetchQuery;
use crate::api::response::{FetchResponse, HealthResponse, NetworkAsset, SyncAudit};
use crate::error::ExperienceRepoError;
use crate::server::ServerConfig;

/// Application state shared across handlers.
#[derive(Clone)]
pub struct AppState {
    pub store: Arc<Mutex<dyn GeneStore>>,
    pub api_keys: Arc<HashMap<String, String>>,
}

/// Create the router with all routes.
pub fn create_routes(config: ServerConfig) -> Router {
    let store: Arc<Mutex<dyn GeneStore>> = Arc::new(Mutex::new(
        oris_genestore::SqliteGeneStore::open(&config.store_path)
            .expect("failed to open gene store"),
    ));

    let state = AppState {
        store,
        api_keys: Arc::new(config.api_keys),
    };

    Router::new()
        .route("/experience", get(fetch_experiences))
        .route("/health", get(health))
        .with_state(state)
}

/// Handler for GET /experience - fetch matching experiences.
async fn fetch_experiences(
    State(state): State<AppState>,
    Query(query): Query<FetchQuery>,
) -> Result<Json<FetchResponse>, ExperienceRepoError> {
    // Validation would be done via middleware in production
    // For MVP, we skip auth validation here

    let signals = query.signals();
    let limit = query.limit;
    let min_confidence = query.min_confidence;

    // Build gene query
    let gene_query = GeneQuery {
        min_confidence,
        limit,
        required_tags: vec![],
        problem_description: signals.join(","),
    };

    // Search genes
    let store = state.store.lock().await;

    let matches = store.search_genes(&gene_query).await.map_err(|e| {
        ExperienceRepoError::GeneStoreError(anyhow::anyhow!("search failed: {}", e))
    })?;

    drop(store);

    let scanned_count = matches.len();
    let assets: Vec<NetworkAsset> = matches
        .into_iter()
        .map(|m| {
            let gene = m.gene;
            NetworkAsset::Gene {
                id: gene.id.to_string(),
                signals: gene.tags,
                strategy: gene.template.lines().map(|s| s.to_string()).collect(),
                validation: gene.validation_steps,
                confidence: gene.confidence,
                quality_score: gene.quality_score,
                use_count: gene.use_count,
                success_count: gene.success_count,
                created_at: gene.created_at.to_rfc3339(),
            }
        })
        .collect();

    Ok(Json(FetchResponse {
        assets,
        next_cursor: None,
        sync_audit: SyncAudit {
            scanned_count,
            applied_count: scanned_count,
            skipped_count: 0,
            failed_count: 0,
        },
    }))
}

/// Handler for GET /health - health check (no auth required).
async fn health() -> Json<HealthResponse> {
    Json(HealthResponse::ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use oris_genestore::SqliteGeneStore;

    fn create_state() -> AppState {
        let store = SqliteGeneStore::open(":memory:").unwrap();
        let mut keys = HashMap::new();
        keys.insert("test-key".to_string(), "agent-001".to_string());

        AppState {
            store: Arc::new(Mutex::new(store)),
            api_keys: Arc::new(keys),
        }
    }

    #[tokio::test]
    async fn test_fetch_experiences_empty() {
        let state = create_state();

        let query = FetchQuery {
            q: Some("timeout".to_string()),
            min_confidence: 0.5,
            limit: 10,
            cursor: None,
        };

        let result = fetch_experiences(State(state), Query(query)).await;
        assert!(result.is_ok());
        let response = result.unwrap().0;
        assert!(response.assets.is_empty());
    }

    #[tokio::test]
    async fn test_health() {
        let response = health().await;
        assert_eq!(response.0.status, "ok");
    }
}
