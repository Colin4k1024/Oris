use oris_hub::registry::{
    HeartbeatRequest, NodeStatus, RegisterRequest, RegistryService, RegistryStore,
    SqliteRegistryStore,
};
use std::sync::Arc;

fn make_store() -> Arc<SqliteRegistryStore> {
    Arc::new(SqliteRegistryStore::new(":memory:").unwrap())
}

fn make_service() -> RegistryService {
    RegistryService::new(make_store())
}

fn register_req(id: &str) -> RegisterRequest {
    RegisterRequest {
        node_id: id.to_string(),
        endpoint: format!("http://node-{id}:8080"),
        public_key: "dGVzdC1rZXk=".to_string(),
        capabilities: vec!["gene-store".to_string(), "evolution".to_string()],
        region: Some("us-west-2".to_string()),
        version: "0.3.0".to_string(),
    }
}

#[tokio::test]
async fn register_and_get_node() {
    let svc = make_service();
    let resp = svc.register(register_req("node-1")).await.unwrap();
    assert_eq!(resp.node_id, "node-1");
    assert!(resp.ttl_seconds > 0);

    let node = svc.get_node("node-1").await.unwrap();
    assert_eq!(node.node_id, "node-1");
    assert_eq!(node.endpoint, "http://node-node-1:8080");
    assert_eq!(node.status, NodeStatus::Active);
    assert_eq!(node.capabilities, vec!["gene-store", "evolution"]);
}

#[tokio::test]
async fn register_duplicate_updates_node() {
    let svc = make_service();
    svc.register(register_req("node-dup")).await.unwrap();

    let mut req2 = register_req("node-dup");
    req2.endpoint = "http://updated:9090".to_string();
    svc.register(req2).await.unwrap();

    let node = svc.get_node("node-dup").await.unwrap();
    assert_eq!(node.endpoint, "http://updated:9090");
}

#[tokio::test]
async fn heartbeat_updates_last_seen() {
    let svc = make_service();
    svc.register(register_req("node-hb")).await.unwrap();

    let before = svc.get_node("node-hb").await.unwrap().last_heartbeat;
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;

    let hb_req = HeartbeatRequest {
        node_id: "node-hb".to_string(),
        status: None,
    };
    let resp = svc.heartbeat(hb_req).await.unwrap();
    assert!(resp.acknowledged);

    let after = svc.get_node("node-hb").await.unwrap().last_heartbeat;
    assert!(after >= before);
}

#[tokio::test]
async fn heartbeat_with_status_change() {
    let svc = make_service();
    svc.register(register_req("node-status")).await.unwrap();

    let hb_req = HeartbeatRequest {
        node_id: "node-status".to_string(),
        status: Some(NodeStatus::Degraded),
    };
    svc.heartbeat(hb_req).await.unwrap();

    let node = svc.get_node("node-status").await.unwrap();
    assert_eq!(node.status, NodeStatus::Degraded);
}

#[tokio::test]
async fn heartbeat_unknown_node_returns_error() {
    let svc = make_service();
    let hb_req = HeartbeatRequest {
        node_id: "ghost".to_string(),
        status: None,
    };
    let result = svc.heartbeat(hb_req).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn deregister_removes_node() {
    let svc = make_service();
    svc.register(register_req("node-del")).await.unwrap();
    svc.deregister("node-del").await.unwrap();

    let result = svc.get_node("node-del").await;
    assert!(result.is_err());
}

#[tokio::test]
async fn list_active_nodes_filters_expired() {
    let store = make_store();
    let svc = RegistryService::new(store.clone());

    svc.register(register_req("alive")).await.unwrap();

    // Manually insert an expired node directly via store
    use chrono::Utc;
    use oris_hub::registry::NodeInfo;
    let expired = NodeInfo {
        node_id: "expired".to_string(),
        endpoint: "http://expired:8080".to_string(),
        public_key: "key".to_string(),
        capabilities: vec![],
        region: None,
        version: "0.1.0".to_string(),
        status: NodeStatus::Active,
        registered_at: Utc::now() - chrono::Duration::seconds(200),
        last_heartbeat: Utc::now() - chrono::Duration::seconds(200),
        ttl_seconds: 60,
    };
    store.upsert_node(&expired).await.unwrap();

    let active = svc.list_active_nodes().await.unwrap();
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].node_id, "alive");
}

#[tokio::test]
async fn gc_removes_expired_nodes() {
    let store = make_store();
    let svc = RegistryService::new(store.clone());

    svc.register(register_req("keeper")).await.unwrap();

    use chrono::Utc;
    use oris_hub::registry::NodeInfo;
    let expired = NodeInfo {
        node_id: "goner".to_string(),
        endpoint: "http://goner:8080".to_string(),
        public_key: "key".to_string(),
        capabilities: vec![],
        region: None,
        version: "0.1.0".to_string(),
        status: NodeStatus::Active,
        registered_at: Utc::now() - chrono::Duration::seconds(200),
        last_heartbeat: Utc::now() - chrono::Duration::seconds(200),
        ttl_seconds: 60,
    };
    store.upsert_node(&expired).await.unwrap();

    let removed = svc.gc().await.unwrap();
    assert_eq!(removed, 1);

    // keeper should still exist
    let node = svc.get_node("keeper").await.unwrap();
    assert_eq!(node.node_id, "keeper");
}
