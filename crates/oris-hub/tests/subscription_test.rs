use axum::{routing::post, Json, Router};
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::sync::Mutex;

use oris_hub::subscription::{
    CreateSubscriptionRequest, GenePromotedEvent, SubscriptionFilter, SubscriptionManager,
    SubscriptionStore, WebhookDispatcher,
};

fn make_manager() -> SubscriptionManager {
    let store = Arc::new(SubscriptionStore::new(":memory:").unwrap());
    let dispatcher = Arc::new(WebhookDispatcher::new());
    SubscriptionManager::new(store, dispatcher)
}

fn make_event(gene_id: &str, task_class: &str, confidence: f64, source: &str) -> GenePromotedEvent {
    GenePromotedEvent {
        gene_id: gene_id.to_string(),
        gene_name: format!("Gene {gene_id}"),
        task_class: task_class.to_string(),
        confidence,
        source_node_id: source.to_string(),
        promoted_at: chrono::Utc::now(),
    }
}

#[test]
fn subscription_store_crud() {
    let store = SubscriptionStore::new(":memory:").unwrap();

    let sub = store
        .create(&CreateSubscriptionRequest {
            subscriber_node_id: "node-1".to_string(),
            callback_url: "http://localhost:9999/hook".to_string(),
            filter: SubscriptionFilter {
                task_class: Some("build-fix".to_string()),
                min_confidence: Some(0.8),
                source_nodes: None,
            },
        })
        .unwrap();

    assert!(!sub.id.is_empty());
    assert_eq!(sub.subscriber_node_id, "node-1");
    assert!(sub.active);

    let fetched = store.get(&sub.id).unwrap().unwrap();
    assert_eq!(fetched.id, sub.id);
    assert_eq!(fetched.filter.task_class, Some("build-fix".to_string()));

    let all = store.list(None).unwrap();
    assert_eq!(all.len(), 1);

    let by_node = store.list(Some("node-1")).unwrap();
    assert_eq!(by_node.len(), 1);

    let by_other = store.list(Some("node-2")).unwrap();
    assert_eq!(by_other.len(), 0);

    store.delete(&sub.id).unwrap();
    let after_delete = store.list(None).unwrap();
    assert_eq!(after_delete.len(), 0);
}

#[test]
fn subscription_store_multiple() {
    let store = SubscriptionStore::new(":memory:").unwrap();

    for i in 0..3 {
        store
            .create(&CreateSubscriptionRequest {
                subscriber_node_id: format!("node-{i}"),
                callback_url: format!("http://node-{i}:8080/hook"),
                filter: SubscriptionFilter {
                    task_class: None,
                    min_confidence: None,
                    source_nodes: None,
                },
            })
            .unwrap();
    }

    let all = store.list_active().unwrap();
    assert_eq!(all.len(), 3);
}

#[tokio::test]
async fn manager_filter_matches_task_class() {
    let mgr = make_manager();

    mgr.create(&CreateSubscriptionRequest {
        subscriber_node_id: "n1".to_string(),
        callback_url: "http://unreachable:1234/hook".to_string(),
        filter: SubscriptionFilter {
            task_class: Some("build-fix".to_string()),
            min_confidence: None,
            source_nodes: None,
        },
    })
    .unwrap();

    mgr.create(&CreateSubscriptionRequest {
        subscriber_node_id: "n2".to_string(),
        callback_url: "http://unreachable:1234/hook".to_string(),
        filter: SubscriptionFilter {
            task_class: Some("test-fix".to_string()),
            min_confidence: None,
            source_nodes: None,
        },
    })
    .unwrap();

    let event = make_event("g1", "build-fix", 0.9, "src-node");
    let result = mgr.notify_gene_promoted(event).await.unwrap();
    assert_eq!(result.total_matched, 1);
}

#[tokio::test]
async fn manager_filter_matches_min_confidence() {
    let mgr = make_manager();

    mgr.create(&CreateSubscriptionRequest {
        subscriber_node_id: "n1".to_string(),
        callback_url: "http://unreachable:1234/hook".to_string(),
        filter: SubscriptionFilter {
            task_class: None,
            min_confidence: Some(0.9),
            source_nodes: None,
        },
    })
    .unwrap();

    let low_conf = make_event("g1", "build-fix", 0.5, "src");
    let result = mgr.notify_gene_promoted(low_conf).await.unwrap();
    assert_eq!(result.total_matched, 0);

    let high_conf = make_event("g2", "build-fix", 0.95, "src");
    let result = mgr.notify_gene_promoted(high_conf).await.unwrap();
    assert_eq!(result.total_matched, 1);
}

#[tokio::test]
async fn manager_filter_matches_source_nodes() {
    let mgr = make_manager();

    mgr.create(&CreateSubscriptionRequest {
        subscriber_node_id: "n1".to_string(),
        callback_url: "http://unreachable:1234/hook".to_string(),
        filter: SubscriptionFilter {
            task_class: None,
            min_confidence: None,
            source_nodes: Some(vec!["allowed-node".to_string()]),
        },
    })
    .unwrap();

    let from_allowed = make_event("g1", "build-fix", 0.9, "allowed-node");
    let result = mgr.notify_gene_promoted(from_allowed).await.unwrap();
    assert_eq!(result.total_matched, 1);

    let from_other = make_event("g2", "build-fix", 0.9, "other-node");
    let result = mgr.notify_gene_promoted(from_other).await.unwrap();
    assert_eq!(result.total_matched, 0);
}

async fn start_webhook_receiver(port: u16) -> (String, Arc<Mutex<Vec<GenePromotedEvent>>>) {
    let received = Arc::new(Mutex::new(Vec::new()));
    let received_clone = Arc::clone(&received);

    let app = Router::new().route(
        "/hook",
        post(move |Json(event): Json<GenePromotedEvent>| {
            let store = Arc::clone(&received_clone);
            async move {
                store.lock().await.push(event);
                Json(serde_json::json!({"ok": true}))
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

    (format!("http://{addr}/hook"), received)
}

#[tokio::test]
async fn webhook_delivery_success() {
    let (url, received) = start_webhook_receiver(0).await;

    let store = Arc::new(SubscriptionStore::new(":memory:").unwrap());
    let dispatcher = Arc::new(WebhookDispatcher::new());
    let mgr = SubscriptionManager::new(store, dispatcher);

    mgr.create(&CreateSubscriptionRequest {
        subscriber_node_id: "n1".to_string(),
        callback_url: url,
        filter: SubscriptionFilter {
            task_class: None,
            min_confidence: None,
            source_nodes: None,
        },
    })
    .unwrap();

    let event = make_event("g-push", "build-fix", 0.95, "origin");
    let result = mgr.notify_gene_promoted(event).await.unwrap();

    assert_eq!(result.total_matched, 1);
    assert_eq!(result.delivered, 1);
    assert_eq!(result.failed, 0);

    tokio::time::sleep(Duration::from_millis(50)).await;
    let events = received.lock().await;
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].gene_id, "g-push");
}

#[tokio::test]
async fn webhook_delivery_failure_unreachable() {
    let store = Arc::new(SubscriptionStore::new(":memory:").unwrap());
    let dispatcher = Arc::new(WebhookDispatcher::new().with_max_retries(0));
    let mgr = SubscriptionManager::new(store, dispatcher);

    mgr.create(&CreateSubscriptionRequest {
        subscriber_node_id: "n1".to_string(),
        callback_url: "http://127.0.0.1:1/hook".to_string(),
        filter: SubscriptionFilter {
            task_class: None,
            min_confidence: None,
            source_nodes: None,
        },
    })
    .unwrap();

    let event = make_event("g-fail", "build-fix", 0.95, "origin");
    let result = mgr.notify_gene_promoted(event).await.unwrap();

    assert_eq!(result.total_matched, 1);
    assert_eq!(result.delivered, 0);
    assert_eq!(result.failed, 1);
}
