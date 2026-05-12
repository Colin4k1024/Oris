use oris_hub::discovery::{DiscoveryQuery, DiscoveryService};
use oris_hub::registry::{RegisterRequest, RegistryService, SqliteRegistryStore};
use std::sync::Arc;

fn make_service() -> (Arc<RegistryService>, DiscoveryService) {
    let store = Arc::new(SqliteRegistryStore::new(":memory:").unwrap());
    let registry = Arc::new(RegistryService::new(store));
    let discovery = DiscoveryService::new(Arc::clone(&registry));
    (registry, discovery)
}

fn register_req(id: &str, caps: Vec<&str>, region: Option<&str>, version: &str) -> RegisterRequest {
    RegisterRequest {
        node_id: id.to_string(),
        endpoint: format!("http://{id}:8080"),
        public_key: "key".to_string(),
        capabilities: caps.into_iter().map(String::from).collect(),
        region: region.map(String::from),
        version: version.to_string(),
    }
}

#[tokio::test]
async fn discover_all_active_nodes() {
    let (reg, disc) = make_service();
    reg.register(register_req(
        "n1",
        vec!["gene-store"],
        Some("us-west"),
        "0.3.0",
    ))
    .await
    .unwrap();
    reg.register(register_req(
        "n2",
        vec!["evolution"],
        Some("eu-west"),
        "0.3.0",
    ))
    .await
    .unwrap();

    let result = disc
        .discover(DiscoveryQuery {
            capabilities: None,
            region: None,
            version: None,
            limit: None,
        })
        .await
        .unwrap();

    assert_eq!(result.total, 2);
    assert_eq!(result.nodes.len(), 2);
}

#[tokio::test]
async fn discover_filter_by_capability() {
    let (reg, disc) = make_service();
    reg.register(register_req(
        "n1",
        vec!["gene-store", "evolution"],
        None,
        "0.3.0",
    ))
    .await
    .unwrap();
    reg.register(register_req("n2", vec!["gene-store"], None, "0.3.0"))
        .await
        .unwrap();
    reg.register(register_req("n3", vec!["sandbox"], None, "0.3.0"))
        .await
        .unwrap();

    let result = disc
        .discover(DiscoveryQuery {
            capabilities: Some(vec!["gene-store".to_string()]),
            region: None,
            version: None,
            limit: None,
        })
        .await
        .unwrap();

    assert_eq!(result.total, 2);
    let ids: Vec<&str> = result.nodes.iter().map(|n| n.node_id.as_str()).collect();
    assert!(ids.contains(&"n1"));
    assert!(ids.contains(&"n2"));
}

#[tokio::test]
async fn discover_filter_by_region() {
    let (reg, disc) = make_service();
    reg.register(register_req("n1", vec!["a"], Some("us-west"), "0.3.0"))
        .await
        .unwrap();
    reg.register(register_req("n2", vec!["a"], Some("eu-west"), "0.3.0"))
        .await
        .unwrap();

    let result = disc
        .discover(DiscoveryQuery {
            capabilities: None,
            region: Some("eu-west".to_string()),
            version: None,
            limit: None,
        })
        .await
        .unwrap();

    assert_eq!(result.total, 1);
    assert_eq!(result.nodes[0].node_id, "n2");
}

#[tokio::test]
async fn discover_filter_by_version() {
    let (reg, disc) = make_service();
    reg.register(register_req("n1", vec!["a"], None, "0.3.0"))
        .await
        .unwrap();
    reg.register(register_req("n2", vec!["a"], None, "0.4.0"))
        .await
        .unwrap();

    let result = disc
        .discover(DiscoveryQuery {
            capabilities: None,
            region: None,
            version: Some("0.4.0".to_string()),
            limit: None,
        })
        .await
        .unwrap();

    assert_eq!(result.total, 1);
    assert_eq!(result.nodes[0].node_id, "n2");
}

#[tokio::test]
async fn discover_with_limit() {
    let (reg, disc) = make_service();
    for i in 0..5 {
        reg.register(register_req(&format!("n{i}"), vec!["a"], None, "0.3.0"))
            .await
            .unwrap();
    }

    let result = disc
        .discover(DiscoveryQuery {
            capabilities: None,
            region: None,
            version: None,
            limit: Some(3),
        })
        .await
        .unwrap();

    assert_eq!(result.total, 5);
    assert_eq!(result.nodes.len(), 3);
}

#[tokio::test]
async fn discover_combined_filters() {
    let (reg, disc) = make_service();
    reg.register(register_req(
        "n1",
        vec!["gene-store", "evolution"],
        Some("us-west"),
        "0.3.0",
    ))
    .await
    .unwrap();
    reg.register(register_req(
        "n2",
        vec!["gene-store"],
        Some("us-west"),
        "0.3.0",
    ))
    .await
    .unwrap();
    reg.register(register_req(
        "n3",
        vec!["gene-store", "evolution"],
        Some("eu-west"),
        "0.3.0",
    ))
    .await
    .unwrap();

    let result = disc
        .discover(DiscoveryQuery {
            capabilities: Some(vec!["evolution".to_string()]),
            region: Some("us-west".to_string()),
            version: None,
            limit: None,
        })
        .await
        .unwrap();

    assert_eq!(result.total, 1);
    assert_eq!(result.nodes[0].node_id, "n1");
}
