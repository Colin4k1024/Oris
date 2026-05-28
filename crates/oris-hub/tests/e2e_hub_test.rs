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

fn test_signing_key() -> SigningKey {
    SigningKey::generate(&mut OsRng)
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

fn sign_legacy_body(key: &SigningKey, body: &[u8]) -> String {
    BASE64.encode(key.sign(body).to_bytes())
}

#[tokio::test]
async fn e2e_register_discover_gc() {
    let app = build_app();

    // Each node gets its own signing key
    let keys: Vec<SigningKey> = (0..3).map(|_| test_signing_key()).collect();

    // Register 3 nodes
    for i in 1..=3 {
        let key = &keys[i - 1];
        let pk_b64 = BASE64.encode(key.verifying_key().as_bytes());
        let body = serde_json::json!({
            "node_id": format!("node-{i}"),
            "endpoint": format!("http://node-{i}:8080"),
            "public_key": pk_b64,
            "capabilities": ["gene-store", "evolution"],
            "region": "us-west-2",
            "version": "0.3.0"
        });

        let body_bytes = serde_json::to_vec(&body).unwrap();
        let ts = Utc::now().timestamp();
        let sig = sign_request(key, "POST", "/hub/nodes", &body_bytes, ts);

        let req = Request::builder()
            .method("POST")
            .uri("/hub/nodes")
            .header("content-type", "application/json")
            .header("x-oen-signature", &sig)
            .header("x-oen-timestamp", ts.to_string())
            .body(Body::from(body_bytes))
            .unwrap();

        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK, "register node-{i} failed");
    }

    // Discover all nodes
    let discover_body = serde_json::json!({
        "capabilities": null,
        "region": null,
        "version": null,
        "limit": null
    });

    let req = Request::builder()
        .method("GET")
        .uri("/hub/nodes")
        .header("content-type", "application/json")
        .header("authorization", "Bearer test-api-key")
        .body(Body::from(serde_json::to_vec(&discover_body).unwrap()))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body_bytes = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let result: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(result["total"], 3);
    assert_eq!(result["nodes"].as_array().unwrap().len(), 3);

    // Discover with capability filter
    let discover_body = serde_json::json!({
        "capabilities": ["gene-store"],
        "region": "us-west-2",
        "version": null,
        "limit": 2
    });

    let req = Request::builder()
        .method("GET")
        .uri("/hub/nodes")
        .header("content-type", "application/json")
        .header("authorization", "Bearer test-api-key")
        .body(Body::from(serde_json::to_vec(&discover_body).unwrap()))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body_bytes = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let result: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(result["total"], 3);
    assert_eq!(result["nodes"].as_array().unwrap().len(), 2);

    // Get single node
    let req = Request::builder()
        .method("GET")
        .uri("/hub/nodes/node-2")
        .header("authorization", "Bearer test-api-key")
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body_bytes = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let node: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(node["node_id"], "node-2");
    assert_eq!(node["endpoint"], "http://node-2:8080");

    // Heartbeat (node-1, key index 0)
    let hb_body = serde_json::json!({
        "node_id": "node-1",
        "status": null
    });
    let hb_bytes = serde_json::to_vec(&hb_body).unwrap();
    let hb_ts = Utc::now().timestamp();
    let hb_sig = sign_request(
        &keys[0],
        "PUT",
        "/hub/nodes/node-1/heartbeat",
        &hb_bytes,
        hb_ts,
    );

    let req = Request::builder()
        .method("PUT")
        .uri("/hub/nodes/node-1/heartbeat")
        .header("content-type", "application/json")
        .header("x-oen-signature", &hb_sig)
        .header("x-oen-timestamp", hb_ts.to_string())
        .body(Body::from(hb_bytes))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Deregister node-3 (key index 2)
    let empty: &[u8] = b"";
    let del_ts = Utc::now().timestamp();
    let del_sig = sign_request(&keys[2], "DELETE", "/hub/nodes/node-3", empty, del_ts);

    let req = Request::builder()
        .method("DELETE")
        .uri("/hub/nodes/node-3")
        .header("x-oen-signature", &del_sig)
        .header("x-oen-timestamp", del_ts.to_string())
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Stats
    let req = Request::builder()
        .method("GET")
        .uri("/hub/stats")
        .header("authorization", "Bearer test-api-key")
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body_bytes = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let stats: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(stats["active_nodes"], 2);
}

#[tokio::test]
async fn e2e_auth_rejected_without_token() {
    let app = build_app();

    let req = Request::builder()
        .method("GET")
        .uri("/hub/stats")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn e2e_signature_rejected_without_header() {
    let app = build_app();

    let body = serde_json::json!({
        "node_id": "node-x",
        "endpoint": "http://x:8080",
        "public_key": "key",
        "capabilities": [],
        "version": "0.1.0"
    });

    let req = Request::builder()
        .method("POST")
        .uri("/hub/nodes")
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn e2e_legacy_body_only_signature_still_works() {
    let app = build_app();

    let key = test_signing_key();
    let pk_b64 = BASE64.encode(key.verifying_key().as_bytes());
    let body = serde_json::json!({
        "node_id": "legacy-node",
        "endpoint": "http://legacy-node:8080",
        "public_key": pk_b64,
        "capabilities": ["gene-store"],
        "region": "us-west-2",
        "version": "0.3.0"
    });
    let body_bytes = serde_json::to_vec(&body).unwrap();
    let sig = sign_legacy_body(&key, &body_bytes);

    let req = Request::builder()
        .method("POST")
        .uri("/hub/nodes")
        .header("content-type", "application/json")
        .header("x-oen-signature", &sig)
        .body(Body::from(body_bytes))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn e2e_future_timestamp_signature_rejected() {
    let app = build_app();

    let key = test_signing_key();
    let pk_b64 = BASE64.encode(key.verifying_key().as_bytes());
    let body = serde_json::json!({
        "node_id": "future-node",
        "endpoint": "http://future-node:8080",
        "public_key": pk_b64,
        "capabilities": ["gene-store"],
        "region": "us-west-2",
        "version": "0.3.0"
    });
    let body_bytes = serde_json::to_vec(&body).unwrap();
    let ts = Utc::now().timestamp() + 120;
    let sig = sign_request(&key, "POST", "/hub/nodes", &body_bytes, ts);

    let req = Request::builder()
        .method("POST")
        .uri("/hub/nodes")
        .header("content-type", "application/json")
        .header("x-oen-signature", &sig)
        .header("x-oen-timestamp", ts.to_string())
        .body(Body::from(body_bytes))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn e2e_subscription_crud() {
    let app = build_app();

    let sub_body = serde_json::json!({
        "subscriber_node_id": "sub-node-1",
        "callback_url": "http://sub-node-1:8080/webhook",
        "filter": {
            "task_class": "build-fix",
            "min_confidence": 0.8,
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

    let body_bytes = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let sub: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(sub["subscriber_node_id"], "sub-node-1");
    assert_eq!(sub["active"], true);
    let sub_id = sub["id"].as_str().unwrap().to_string();

    let req = Request::builder()
        .method("GET")
        .uri("/hub/subscriptions")
        .header("authorization", "Bearer test-api-key")
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body_bytes = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let subs: Vec<serde_json::Value> = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(subs.len(), 1);

    let req = Request::builder()
        .method("DELETE")
        .uri(format!("/hub/subscriptions/{sub_id}"))
        .header("authorization", "Bearer test-api-key")
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let req = Request::builder()
        .method("GET")
        .uri("/hub/subscriptions")
        .header("authorization", "Bearer test-api-key")
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body_bytes = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let subs: Vec<serde_json::Value> = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(subs.len(), 0);
}

#[tokio::test]
async fn e2e_gene_promoted_event() {
    let app = build_app();

    // Create subscription via authenticated route
    let sub_body = serde_json::json!({
        "subscriber_node_id": "listener",
        "callback_url": "http://unreachable-node:1/unreachable",
        "filter": {
            "task_class": "build-fix",
            "min_confidence": null,
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

    // Register origin-node so its public key is stored
    let origin_key = test_signing_key();
    let origin_pk = BASE64.encode(origin_key.verifying_key().as_bytes());
    let reg_body = serde_json::json!({
        "node_id": "origin-node",
        "endpoint": "http://origin:8080",
        "public_key": origin_pk,
        "capabilities": ["evolution"],
        "region": "us-west-2",
        "version": "0.3.0"
    });
    let reg_bytes = serde_json::to_vec(&reg_body).unwrap();
    let reg_ts = Utc::now().timestamp();
    let reg_sig = sign_request(&origin_key, "POST", "/hub/nodes", &reg_bytes, reg_ts);

    let req = Request::builder()
        .method("POST")
        .uri("/hub/nodes")
        .header("content-type", "application/json")
        .header("x-oen-signature", &reg_sig)
        .header("x-oen-timestamp", reg_ts.to_string())
        .body(Body::from(reg_bytes))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Post gene_promoted event signed by origin-node
    let event_body = serde_json::json!({
        "gene_id": "gene-001",
        "gene_name": "Fix null pointer",
        "task_class": "build-fix",
        "confidence": 0.95,
        "source_node_id": "origin-node",
        "promoted_at": "2026-05-12T10:00:00Z"
    });
    let event_bytes = serde_json::to_vec(&event_body).unwrap();
    let event_ts = Utc::now().timestamp();
    let event_sig = sign_request(
        &origin_key,
        "POST",
        "/hub/events/gene_promoted",
        &event_bytes,
        event_ts,
    );

    let req = Request::builder()
        .method("POST")
        .uri("/hub/events/gene_promoted")
        .header("content-type", "application/json")
        .header("x-oen-signature", &event_sig)
        .header("x-oen-timestamp", event_ts.to_string())
        .body(Body::from(event_bytes))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body_bytes = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let result: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(result["total_matched"], 1);
    assert_eq!(result["failed"], 1);
}
