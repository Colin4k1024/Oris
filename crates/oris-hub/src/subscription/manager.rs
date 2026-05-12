use std::sync::Arc;

use super::dispatcher::WebhookDispatcher;
use super::store::SubscriptionStore;
use super::types::{CreateSubscriptionRequest, GenePromotedEvent, Subscription};
use crate::error::HubError;

pub struct SubscriptionManager {
    store: Arc<SubscriptionStore>,
    dispatcher: Arc<WebhookDispatcher>,
}

impl SubscriptionManager {
    pub fn new(store: Arc<SubscriptionStore>, dispatcher: Arc<WebhookDispatcher>) -> Self {
        Self { store, dispatcher }
    }

    pub fn create(&self, req: &CreateSubscriptionRequest) -> Result<Subscription, HubError> {
        self.store.create(req)
    }

    pub fn list(&self, subscriber_node_id: Option<&str>) -> Result<Vec<Subscription>, HubError> {
        self.store.list(subscriber_node_id)
    }

    pub fn delete(&self, id: &str) -> Result<(), HubError> {
        self.store.delete(id)
    }

    pub fn get(&self, id: &str) -> Result<Option<Subscription>, HubError> {
        self.store.get(id)
    }

    pub async fn notify_gene_promoted(
        &self,
        event: GenePromotedEvent,
    ) -> Result<NotifyResult, HubError> {
        let subscriptions = self.store.list_active()?;
        let matched: Vec<&Subscription> = subscriptions
            .iter()
            .filter(|sub| matches_filter(sub, &event))
            .collect();

        let total_matched = matched.len();
        let mut delivered = 0;
        let mut failed = 0;

        for sub in matched {
            let success = self.dispatcher.push(&sub.callback_url, &event).await;
            if success {
                delivered += 1;
            } else {
                failed += 1;
            }
        }

        Ok(NotifyResult {
            total_matched,
            delivered,
            failed,
        })
    }
}

fn matches_filter(sub: &Subscription, event: &GenePromotedEvent) -> bool {
    if let Some(ref tc) = sub.filter.task_class {
        if tc != &event.task_class {
            return false;
        }
    }
    if let Some(min_conf) = sub.filter.min_confidence {
        if event.confidence < min_conf {
            return false;
        }
    }
    if let Some(ref nodes) = sub.filter.source_nodes {
        if !nodes.contains(&event.source_node_id) {
            return false;
        }
    }
    true
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct NotifyResult {
    pub total_matched: usize,
    pub delivered: usize,
    pub failed: usize,
}
