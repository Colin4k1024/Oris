use std::time::Duration;
use tracing::{info, warn};

use super::types::GenePromotedEvent;

pub struct WebhookDispatcher {
    client: reqwest::Client,
    max_retries: u32,
}

impl WebhookDispatcher {
    pub fn new() -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .unwrap_or_default();

        Self {
            client,
            max_retries: 3,
        }
    }

    pub fn with_max_retries(mut self, retries: u32) -> Self {
        self.max_retries = retries;
        self
    }

    pub async fn push(&self, callback_url: &str, event: &GenePromotedEvent) -> bool {
        for attempt in 0..=self.max_retries {
            if attempt > 0 {
                let backoff = Duration::from_millis(500 * 2u64.pow(attempt - 1));
                tokio::time::sleep(backoff).await;
            }

            match self.client.post(callback_url).json(event).send().await {
                Ok(resp) if resp.status().is_success() => {
                    info!(
                        url = callback_url,
                        gene_id = %event.gene_id,
                        attempt = attempt + 1,
                        "webhook delivered"
                    );
                    return true;
                }
                Ok(resp) => {
                    warn!(
                        url = callback_url,
                        status = %resp.status(),
                        attempt = attempt + 1,
                        "webhook non-success response"
                    );
                }
                Err(e) => {
                    warn!(
                        url = callback_url,
                        error = %e,
                        attempt = attempt + 1,
                        "webhook delivery failed"
                    );
                }
            }
        }

        warn!(
            url = callback_url,
            gene_id = %event.gene_id,
            "webhook delivery exhausted all retries"
        );
        false
    }
}
