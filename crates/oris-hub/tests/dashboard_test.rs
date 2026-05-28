use axum::body::Body;
use axum::http::{Request, StatusCode};
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use chrono::Utc;
use ed25519_dalek::{Signer, SigningKey};
use rand::rngs::OsRng;
use std::sync::Arc;
use tower::ServiceExt;

use oris_hub::api::{build_router, AppState};
use oris_hub::discovery::DiscoveryService;
use oris_hub::federation::FederationEngine;
use oris_hub::middleware::TokenStore;
use oris_hub::registry::{RegistryService, SqliteRegistryStore};
use oris_hub::subscription::{SubscriptionManager, SubscriptionStore, WebhookDispatcher};

fn build_app() -> axum::Router {
    let store = Arc::new(SqliteRegistryStore::new(":memory:").unwrap());
    let registry = Arc::new(RegistryService::new(store));
    let discovery = DiscoveryService::new(Arc::clone(&registry));
    let federation = FederationEngine::new(Arc::clone(&registry));
    let sub_store = Arc::new(SubscriptionStore::new(":memory:").unwrap());
    let dispatcher = Arc::new(WebhookDispatcher::new());
    let subscriptions = SubscriptionManager::new(sub_store, dispatcher);

    let state = Arc::new(AppState {
        registry,
        discovery,
        federation,
        subscriptions,
        token_store: TokenStore::with_tokens(vec!["test-api-key".to_string()]),
        signature_max_age_seconds: 300,
    });

    build_router(state)
}

fn sign_request(key: &SigningKey, method: &str, path: &str, body: &[u8], timestamp: i64) -> String {
    let mut payload = timestamp.to_string().into_bytes();
    payload.push(b'\n');
    payload.extend_from_slice(method.as_bytes());
    payload.push(b'\n');
    payload.extend_from_slice(path.as_bytes());
    payload.push(b'\n');
    payload.extend_from_slice(body);
    BASE64.encode(key.sign(&payload).to_bytes())
}

#[tokio::test]
async fn dashboard_overview_accessible() {
    let app = build_app();

    let req = Request::builder()
        .method("GET")
        .uri("/dashboard")
        .header("authorization", "Bearer test-api-key")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let html = String::from_utf8(body.to_vec()).unwrap();
    assert!(html.contains("Oris Hub"));
    assert!(html.contains("Active Nodes"));
    assert!(html.contains("Subscriptions"));
}

#[tokio::test]
async fn dashboard_nodes_page() {
    let app = build_app();

    let key = SigningKey::generate(&mut OsRng);
    let pk_b64 = BASE64.encode(key.verifying_key().as_bytes());
    let body = serde_json::json!({
        "node_id": "dash-node-1",
        "endpoint": "http://dash-node-1:8080",
        "public_key": pk_b64,
        "capabilities": ["gene-store"],
        "region": "eu-west-1",
        "version": "0.3.0"
    });
    let body_bytes = serde_json::to_vec(&body).unwrap();
    let ts = Utc::now().timestamp();
    let sig = sign_request(&key, "POST", "/hub/nodes", &body_bytes, ts);

    let req = Request::builder()
        .method("POST")
        .uri("/hub/nodes")
        .header("content-type", "application/json")
        .header("x-oen-signature", &sig)
        .header("x-oen-timestamp", ts.to_string())
        .body(Body::from(body_bytes))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let req = Request::builder()
        .method("GET")
        .uri("/dashboard/nodes")
        .header("authorization", "Bearer test-api-key")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let html = String::from_utf8(body.to_vec()).unwrap();
    assert!(html.contains("dash-node-1"));
    assert!(html.contains("eu-west-1"));
    assert!(html.contains("gene-store"));
}

#[tokio::test]
async fn dashboard_subscriptions_page() {
    let app = build_app();

    let sub_body = serde_json::json!({
        "subscriber_node_id": "dash-sub-1",
        "callback_url": "http://dash-sub-1:8080/hook",
        "filter": {
            "task_class": "build-fix",
            "min_confidence": 0.85,
            "source_nodes": null
        }
    });

    let req = Request::builder()
        .method("POST")
        .uri("/hub/subscriptions")
        .header("content-type", "application/json")
        .header("authorization", "Bearer test-api-key")
        .body(Body::from(serde_json::to_vec(&sub_body).unwrap()))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let req = Request::builder()
        .method("GET")
        .uri("/dashboard/subscriptions")
        .header("authorization", "Bearer test-api-key")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let html = String::from_utf8(body.to_vec()).unwrap();
    assert!(html.contains("dash-sub-1"));
    assert!(html.contains("build-fix"));
    assert!(html.contains("0.85"));
}

#[tokio::test]
async fn dashboard_node_detail_page() {
    let app = build_app();

    let key = SigningKey::generate(&mut OsRng);
    let pk_b64 = BASE64.encode(key.verifying_key().as_bytes());
    let body = serde_json::json!({
        "node_id": "detail-node-1",
        "endpoint": "http://detail-node-1:9090",
        "public_key": pk_b64,
        "capabilities": ["gene-store", "mutation"],
        "region": "ap-east-1",
        "version": "0.4.0"
    });
    let body_bytes = serde_json::to_vec(&body).unwrap();
    let ts = Utc::now().timestamp();
    let sig = sign_request(&key, "POST", "/hub/nodes", &body_bytes, ts);

    let req = Request::builder()
        .method("POST")
        .uri("/hub/nodes")
        .header("content-type", "application/json")
        .header("x-oen-signature", &sig)
        .header("x-oen-timestamp", ts.to_string())
        .body(Body::from(body_bytes))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let req = Request::builder()
        .method("GET")
        .uri("/dashboard/nodes/detail-node-1")
        .header("authorization", "Bearer test-api-key")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let html = String::from_utf8(body.to_vec()).unwrap();
    assert!(html.contains("detail-node-1"));
    assert!(html.contains("ap-east-1"));
    assert!(html.contains("gene-store"));
    assert!(html.contains("mutation"));
    assert!(html.contains("0.4.0"));
    assert!(html.contains("Active"));
}

#[tokio::test]
async fn dashboard_node_not_found() {
    let app = build_app();

    let req = Request::builder()
        .method("GET")
        .uri("/dashboard/nodes/nonexistent-node")
        .header("authorization", "Bearer test-api-key")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let html = String::from_utf8(body.to_vec()).unwrap();
    assert!(html.contains("Node Not Found"));
    assert!(html.contains("nonexistent-node"));
}

#[tokio::test]
async fn dashboard_search_page() {
    let app = build_app();

    let req = Request::builder()
        .method("GET")
        .uri("/dashboard/search")
        .header("authorization", "Bearer test-api-key")
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let html = String::from_utf8(body.to_vec()).unwrap();
    assert!(html.contains("Federated Search"));
    assert!(html.contains("Search"));
}

#[tokio::test]
async fn dashboard_search_with_query() {
    let app = build_app();

    let req = Request::builder()
        .method("GET")
        .uri("/dashboard/search?q=build-fix&task_class=build-fix")
        .header("authorization", "Bearer test-api-key")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let html = String::from_utf8(body.to_vec()).unwrap();
    assert!(html.contains("build-fix"));
    assert!(html.contains("Results for"));
}

#[tokio::test]
async fn dashboard_requires_auth() {
    let app = build_app();

    let paths = [
        "/dashboard",
        "/dashboard/nodes",
        "/dashboard/subscriptions",
        "/dashboard/search",
    ];

    for path in paths {
        let req = Request::builder()
            .method("GET")
            .uri(path)
            .body(Body::empty())
            .unwrap();

        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::UNAUTHORIZED,
            "dashboard path {path} should require auth"
        );

        let req = Request::builder()
            .method("GET")
            .uri(path)
            .header("authorization", "Bearer test-api-key")
            .body(Body::empty())
            .unwrap();

        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "dashboard path {path} should be accessible with auth"
        );
    }
}
