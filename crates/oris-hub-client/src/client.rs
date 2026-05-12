use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tracing::{info, warn};

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use ed25519_dalek::{Signer, SigningKey};

use oris_hub::discovery::{DiscoveryQuery, DiscoveryResult};
use oris_hub::federation::{FederatedQuery, FederatedResult};
use oris_hub::registry::{HeartbeatRequest, HeartbeatResponse, RegisterRequest, RegisterResponse};
use oris_hub::subscription::{CreateSubscriptionRequest, Subscription};

use crate::error::HubClientError;

#[derive(Debug, Clone)]
pub struct HubClientConfig {
    pub hub_url: String,
    pub node_id: String,
    pub endpoint: String,
    pub capabilities: Vec<String>,
    pub region: Option<String>,
    pub version: String,
    pub api_key: String,
    pub heartbeat_interval: Duration,
}

pub struct HubClient {
    config: HubClientConfig,
    http: reqwest::Client,
    signing_key: SigningKey,
    registered: Arc<RwLock<bool>>,
}

impl HubClient {
    pub fn new(config: HubClientConfig, signing_key: SigningKey) -> Self {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .expect("failed to build HTTP client");

        Self {
            config,
            http,
            signing_key,
            registered: Arc::new(RwLock::new(false)),
        }
    }

    pub async fn register(&self) -> Result<RegisterResponse, HubClientError> {
        let req = RegisterRequest {
            node_id: self.config.node_id.clone(),
            endpoint: self.config.endpoint.clone(),
            public_key: BASE64.encode(self.signing_key.verifying_key().as_bytes()),
            capabilities: self.config.capabilities.clone(),
            region: self.config.region.clone(),
            version: self.config.version.clone(),
        };

        let body =
            serde_json::to_vec(&req).map_err(|e| HubClientError::Serialization(e.to_string()))?;
        let signature = self.sign(&body);

        let resp = self
            .http
            .post(format!("{}/hub/nodes", self.config.hub_url))
            .header("X-OEN-Signature", &signature)
            .header("Content-Type", "application/json")
            .body(body)
            .send()
            .await?;

        if resp.status().is_success() {
            let result: RegisterResponse = resp.json().await?;
            *self.registered.write().await = true;
            info!(node_id = %self.config.node_id, "registered with hub");
            Ok(result)
        } else {
            let status = resp.status().as_u16();
            let message = resp.text().await.unwrap_or_default();
            Err(HubClientError::HubError { status, message })
        }
    }

    pub async fn heartbeat(&self) -> Result<HeartbeatResponse, HubClientError> {
        if !*self.registered.read().await {
            return Err(HubClientError::NotRegistered);
        }

        let req = HeartbeatRequest {
            node_id: self.config.node_id.clone(),
            status: None,
        };

        let body =
            serde_json::to_vec(&req).map_err(|e| HubClientError::Serialization(e.to_string()))?;
        let signature = self.sign(&body);

        let resp = self
            .http
            .put(format!(
                "{}/hub/nodes/{}/heartbeat",
                self.config.hub_url, self.config.node_id
            ))
            .header("X-OEN-Signature", &signature)
            .header("Content-Type", "application/json")
            .body(body)
            .send()
            .await?;

        if resp.status().is_success() {
            Ok(resp.json().await?)
        } else {
            let status = resp.status().as_u16();
            let message = resp.text().await.unwrap_or_default();
            Err(HubClientError::HubError { status, message })
        }
    }

    pub async fn start_heartbeat_loop(self: Arc<Self>) {
        let interval = self.config.heartbeat_interval;
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(interval);
            loop {
                ticker.tick().await;
                if let Err(e) = self.heartbeat().await {
                    warn!(error = %e, "heartbeat failed");
                }
            }
        });
    }

    pub async fn discover(&self, query: DiscoveryQuery) -> Result<DiscoveryResult, HubClientError> {
        let resp = self
            .http
            .get(format!("{}/hub/nodes", self.config.hub_url))
            .header("Authorization", format!("Bearer {}", self.config.api_key))
            .json(&query)
            .send()
            .await?;

        if resp.status().is_success() {
            Ok(resp.json().await?)
        } else {
            let status = resp.status().as_u16();
            let message = resp.text().await.unwrap_or_default();
            Err(HubClientError::HubError { status, message })
        }
    }

    pub async fn search(&self, query: FederatedQuery) -> Result<FederatedResult, HubClientError> {
        let resp = self
            .http
            .post(format!("{}/hub/search", self.config.hub_url))
            .header("Authorization", format!("Bearer {}", self.config.api_key))
            .json(&query)
            .send()
            .await?;

        if resp.status().is_success() {
            Ok(resp.json().await?)
        } else {
            let status = resp.status().as_u16();
            let message = resp.text().await.unwrap_or_default();
            Err(HubClientError::HubError { status, message })
        }
    }

    pub async fn subscribe(
        &self,
        req: &CreateSubscriptionRequest,
    ) -> Result<Subscription, HubClientError> {
        let resp = self
            .http
            .post(format!("{}/hub/subscriptions", self.config.hub_url))
            .header("Authorization", format!("Bearer {}", self.config.api_key))
            .json(req)
            .send()
            .await?;

        if resp.status().is_success() {
            Ok(resp.json().await?)
        } else {
            let status = resp.status().as_u16();
            let message = resp.text().await.unwrap_or_default();
            Err(HubClientError::HubError { status, message })
        }
    }

    pub async fn unsubscribe(&self, subscription_id: &str) -> Result<(), HubClientError> {
        let resp = self
            .http
            .delete(format!(
                "{}/hub/subscriptions/{}",
                self.config.hub_url, subscription_id
            ))
            .header("Authorization", format!("Bearer {}", self.config.api_key))
            .send()
            .await?;

        if resp.status().is_success() {
            Ok(())
        } else {
            let status = resp.status().as_u16();
            let message = resp.text().await.unwrap_or_default();
            Err(HubClientError::HubError { status, message })
        }
    }

    pub async fn list_subscriptions(&self) -> Result<Vec<Subscription>, HubClientError> {
        let resp = self
            .http
            .get(format!("{}/hub/subscriptions", self.config.hub_url))
            .header("Authorization", format!("Bearer {}", self.config.api_key))
            .send()
            .await?;

        if resp.status().is_success() {
            Ok(resp.json().await?)
        } else {
            let status = resp.status().as_u16();
            let message = resp.text().await.unwrap_or_default();
            Err(HubClientError::HubError { status, message })
        }
    }

    fn sign(&self, data: &[u8]) -> String {
        let sig = self.signing_key.sign(data);
        BASE64.encode(sig.to_bytes())
    }
}
