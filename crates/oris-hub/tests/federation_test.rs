use axum::{routing::post, Json, Router};
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;

use oris_hub::federation::{FederatedQuery, FederationEngine, GeneResult, NodeSearchResponse};
use oris_hub::registry::{RegisterRequest, RegistryService, SqliteRegistryStore};

async fn start_mock_node(port: u16, genes: Vec<GeneResult>, delay_ms: Option<u64>) -> String {
    let app = Router::new().route(
        "/experience/search",
        post(move |_body: Json<serde_json::Value>| {
            let genes = genes.clone();
            let delay = delay_ms;
            async move {
                if let Some(ms) = delay {
                    tokio::time::sleep(Duration::from_millis(ms)).await;
                }
                Json(NodeSearchResponse { genes })
            }
        }),
    );

    let listener = TcpListener::bind(format!("127.0.0.1:{port}"))
        .await
        .unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    format!("http://{addr}")
}

fn make_gene(id: &str, name: &str, confidence: f64, source: &str) -> GeneResult {
    GeneResult {
        gene_id: id.to_string(),
        name: name.to_string(),
        task_class: "build-fix".to_string(),
        confidence,
        source_node: source.to_string(),
        created_at: chrono::Utc::now(),
    }
}

#[tokio::test]
async fn federation_aggregates_from_multiple_nodes() {
    let endpoint1 =
        start_mock_node(0, vec![make_gene("g1", "Fix panic", 0.9, "node-1")], None).await;
    let endpoint2 =
        start_mock_node(0, vec![make_gene("g2", "Add retry", 0.8, "node-2")], None).await;

    let store = Arc::new(SqliteRegistryStore::new(":memory:").unwrap());
    let registry = Arc::new(RegistryService::new(store));

    registry
        .register(RegisterRequest {
            node_id: "node-1".to_string(),
            endpoint: endpoint1,
            public_key: "key1".to_string(),
            capabilities: vec!["gene-store".to_string()],
            region: None,
            version: "0.3.0".to_string(),
        })
        .await
        .unwrap();

    registry
        .register(RegisterRequest {
            node_id: "node-2".to_string(),
            endpoint: endpoint2,
            public_key: "key2".to_string(),
            capabilities: vec!["gene-store".to_string()],
            region: None,
            version: "0.3.0".to_string(),
        })
        .await
        .unwrap();

    let engine = FederationEngine::new(registry);
    let result = engine
        .search(FederatedQuery {
            query: "fix panic".to_string(),
            task_class: None,
            min_confidence: None,
            timeout_ms: Some(2000),
            target_nodes: None,
            limit: None,
        })
        .await
        .unwrap();

    assert_eq!(result.results.len(), 2);
    assert_eq!(result.meta.nodes_queried, 2);
    assert_eq!(result.meta.nodes_responded, 2);
    assert!((result.meta.coverage - 1.0).abs() < 0.01);
    // sorted by confidence desc
    assert_eq!(result.results[0].gene_id, "g1");
    assert_eq!(result.results[1].gene_id, "g2");
}

#[tokio::test]
async fn federation_deduplicates_same_gene_id() {
    let endpoint1 = start_mock_node(
        0,
        vec![make_gene("g-shared", "Shared fix", 0.85, "node-1")],
        None,
    )
    .await;
    let endpoint2 = start_mock_node(
        0,
        vec![make_gene("g-shared", "Shared fix", 0.90, "node-2")],
        None,
    )
    .await;

    let store = Arc::new(SqliteRegistryStore::new(":memory:").unwrap());
    let registry = Arc::new(RegistryService::new(store));

    registry
        .register(RegisterRequest {
            node_id: "node-1".to_string(),
            endpoint: endpoint1,
            public_key: "k".to_string(),
            capabilities: vec![],
            region: None,
            version: "0.3.0".to_string(),
        })
        .await
        .unwrap();
    registry
        .register(RegisterRequest {
            node_id: "node-2".to_string(),
            endpoint: endpoint2,
            public_key: "k".to_string(),
            capabilities: vec![],
            region: None,
            version: "0.3.0".to_string(),
        })
        .await
        .unwrap();

    let engine = FederationEngine::new(registry);
    let result = engine
        .search(FederatedQuery {
            query: "shared".to_string(),
            task_class: None,
            min_confidence: None,
            timeout_ms: Some(2000),
            target_nodes: None,
            limit: None,
        })
        .await
        .unwrap();

    assert_eq!(result.results.len(), 1);
    assert_eq!(result.results[0].gene_id, "g-shared");
}

#[tokio::test]
async fn federation_handles_timeout_gracefully() {
    let endpoint_fast = start_mock_node(
        0,
        vec![make_gene("g-fast", "Fast result", 0.95, "fast-node")],
        None,
    )
    .await;
    let endpoint_slow = start_mock_node(
        0,
        vec![make_gene("g-slow", "Slow result", 0.7, "slow-node")],
        Some(3000), // 3s delay — will timeout
    )
    .await;

    let store = Arc::new(SqliteRegistryStore::new(":memory:").unwrap());
    let registry = Arc::new(RegistryService::new(store));

    registry
        .register(RegisterRequest {
            node_id: "fast-node".to_string(),
            endpoint: endpoint_fast,
            public_key: "k".to_string(),
            capabilities: vec![],
            region: None,
            version: "0.3.0".to_string(),
        })
        .await
        .unwrap();
    registry
        .register(RegisterRequest {
            node_id: "slow-node".to_string(),
            endpoint: endpoint_slow,
            public_key: "k".to_string(),
            capabilities: vec![],
            region: None,
            version: "0.3.0".to_string(),
        })
        .await
        .unwrap();

    let engine = FederationEngine::new(registry);
    let result = engine
        .search(FederatedQuery {
            query: "test".to_string(),
            task_class: None,
            min_confidence: None,
            timeout_ms: Some(200), // 200ms timeout
            target_nodes: None,
            limit: None,
        })
        .await
        .unwrap();

    // Fast node responded, slow node timed out
    assert_eq!(result.results.len(), 1);
    assert_eq!(result.results[0].gene_id, "g-fast");
    assert_eq!(result.meta.nodes_queried, 2);
    assert_eq!(result.meta.nodes_responded, 1);
    assert!((result.meta.coverage - 0.5).abs() < 0.01);
    assert!(result.meta.timeout_nodes.contains(&"slow-node".to_string()));
}

#[tokio::test]
async fn federation_target_nodes_filter() {
    let endpoint1 =
        start_mock_node(0, vec![make_gene("g1", "Result 1", 0.9, "node-1")], None).await;
    let endpoint2 =
        start_mock_node(0, vec![make_gene("g2", "Result 2", 0.8, "node-2")], None).await;

    let store = Arc::new(SqliteRegistryStore::new(":memory:").unwrap());
    let registry = Arc::new(RegistryService::new(store));

    registry
        .register(RegisterRequest {
            node_id: "node-1".to_string(),
            endpoint: endpoint1,
            public_key: "k".to_string(),
            capabilities: vec![],
            region: None,
            version: "0.3.0".to_string(),
        })
        .await
        .unwrap();
    registry
        .register(RegisterRequest {
            node_id: "node-2".to_string(),
            endpoint: endpoint2,
            public_key: "k".to_string(),
            capabilities: vec![],
            region: None,
            version: "0.3.0".to_string(),
        })
        .await
        .unwrap();

    let engine = FederationEngine::new(registry);
    let result = engine
        .search(FederatedQuery {
            query: "test".to_string(),
            task_class: None,
            min_confidence: None,
            timeout_ms: Some(2000),
            target_nodes: Some(vec!["node-1".to_string()]),
            limit: None,
        })
        .await
        .unwrap();

    assert_eq!(result.meta.nodes_queried, 1);
    assert_eq!(result.results.len(), 1);
    assert_eq!(result.results[0].source_node, "node-1");
}
