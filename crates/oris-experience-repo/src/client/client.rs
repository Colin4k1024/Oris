//! HTTP client for Experience Repository.

use std::fmt;
use std::time::Duration;

use async_trait::async_trait;
use ed25519_dalek::{Signer, SigningKey};
use hex;
use oris_evolution_network::{EvolutionEnvelope, NetworkPublishError, NetworkPublisher};
use reqwest::Client;
use url::Url;

use crate::api::request::ShareRequest;
pub use crate::api::response::{FetchResponse, NetworkAsset, ShareResponse};
use crate::oen::{MessageType, OenEnvelope};

/// Configuration for the Experience Repository client.
#[derive(Clone)]
pub struct ClientConfig {
    pub base_url: String,
    pub api_key: String,
    /// Optional Ed25519 signing key (raw 32-byte seed) used to sign OEN envelopes.
    pub signing_key: Option<Vec<u8>>,
}

impl fmt::Debug for ClientConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ClientConfig")
            .field("base_url", &self.base_url)
            .field("api_key", &"***REDACTED***")
            .field(
                "signing_key",
                &self.signing_key.as_ref().map(|_| "***REDACTED***"),
            )
            .finish()
    }
}

impl ClientConfig {
    /// Create a new client configuration.
    pub fn new(base_url: impl Into<String>, api_key: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            api_key: api_key.into(),
            signing_key: None,
        }
    }

    /// Attach an Ed25519 signing key (raw 32-byte seed).
    pub fn with_signing_key(mut self, seed: Vec<u8>) -> Self {
        self.signing_key = Some(seed);
        self
    }
}

/// Client for accessing Experience Repository API.
#[derive(Clone)]
pub struct ExperienceRepoClient {
    client: Client,
    config: ClientConfig,
}

impl fmt::Debug for ExperienceRepoClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ExperienceRepoClient")
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

impl ExperienceRepoClient {
    /// Create a new client with configuration.
    pub fn new(config: ClientConfig) -> Result<Self, ClientError> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .connect_timeout(Duration::from_secs(5))
            .build()
            .map_err(|e| ClientError::NetworkError(e))?;
        Ok(Self { client, config })
    }

    /// Fetch experiences matching the given signals.
    pub async fn fetch_experiences(
        &self,
        signals: &[String],
        min_confidence: Option<f64>,
        limit: Option<usize>,
    ) -> Result<FetchResponse, ClientError> {
        let mut url = Url::parse(&self.config.base_url)?.join("/experience")?;

        let query = signals.join(",");
        url.query_pairs_mut()
            .append_pair("q", &query)
            .append_pair("min_confidence", &min_confidence.unwrap_or(0.5).to_string())
            .append_pair("limit", &limit.unwrap_or(10).to_string());

        let response = self
            .client
            .get(url)
            .header("X-Api-Key", &self.config.api_key)
            .send()
            .await?
            .error_for_status()
            .map_err(|e| ClientError::HttpError(e.to_string()))?
            .json::<FetchResponse>()
            .await
            .map_err(|e| ClientError::ParseError(e.to_string()))?;

        Ok(response)
    }

    /// Share an experience by posting an OEN envelope to the repository.
    pub async fn share_experience(
        &self,
        envelope: OenEnvelope,
    ) -> Result<ShareResponse, ClientError> {
        let url = Url::parse(&self.config.base_url)?.join("/experience")?;

        let response = self
            .client
            .post(url)
            .header("X-Api-Key", &self.config.api_key)
            .json(&ShareRequest { envelope })
            .send()
            .await?
            .error_for_status()
            .map_err(|e| ClientError::HttpError(e.to_string()))?
            .json::<ShareResponse>()
            .await
            .map_err(|e| ClientError::ParseError(e.to_string()))?;

        Ok(response)
    }

    /// Check if the server is healthy.
    pub async fn health(&self) -> Result<bool, ClientError> {
        let url = Url::parse(&self.config.base_url)?.join("/health")?;

        let response = self
            .client
            .get(url)
            .header("X-Api-Key", &self.config.api_key)
            .send()
            .await?
            .error_for_status()
            .map_err(|e| ClientError::HttpError(e.to_string()))?;

        Ok(response.status().is_success())
    }
}

/// Implements [`NetworkPublisher`] so `ExperienceRepoClient` can be injected into `EvoKernel`
/// and called at gene-promotion time without the kernel knowing about HTTP details.
#[async_trait]
impl NetworkPublisher for ExperienceRepoClient {
    async fn publish_envelope(
        &self,
        envelope: &EvolutionEnvelope,
    ) -> Result<(), NetworkPublishError> {
        let payload = serde_json::to_value(&envelope.assets)
            .map_err(|e| NetworkPublishError::Serialization(e.to_string()))?;

        // Sign the canonical payload bytes with the configured Ed25519 key.
        let signature = match &self.config.signing_key {
            None => return Err(NetworkPublishError::SigningKeyNotConfigured),
            Some(seed) => {
                let seed_bytes: [u8; 32] = seed.as_slice().try_into().map_err(|_| {
                    NetworkPublishError::Serialization(
                        "signing key must be exactly 32 bytes".into(),
                    )
                })?;
                let signing_key = SigningKey::from_bytes(&seed_bytes);
                let payload_bytes = serde_json::to_vec(&payload)
                    .map_err(|e| NetworkPublishError::Serialization(e.to_string()))?;
                let sig = signing_key.sign(&payload_bytes);
                hex::encode(sig.to_bytes())
            }
        };

        let oen = OenEnvelope {
            sender_id: envelope.sender_id.clone(),
            message_type: MessageType::Publish,
            payload,
            signature,
            timestamp: envelope.timestamp.clone(),
        };
        self.share_experience(oen)
            .await
            .map_err(|e| NetworkPublishError::Http(e.to_string()))?;
        Ok(())
    }
}

/// Client-side errors.
#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("network error: {0}")]
    NetworkError(#[from] reqwest::Error),

    #[error("HTTP error: {0}")]
    HttpError(String),

    #[error("parse error: {0}")]
    ParseError(String),

    #[error("URL error: {0}")]
    UrlError(#[from] url::ParseError),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::oen::MessageType;

    fn sample_envelope() -> OenEnvelope {
        OenEnvelope {
            sender_id: "agent-test".to_string(),
            message_type: MessageType::Publish,
            payload: serde_json::json!({"gene": {}}),
            signature: "sig".to_string(),
            timestamp: "2026-05-11T00:00:00Z".to_string(),
        }
    }

    #[test]
    fn test_client_config() {
        let config = ClientConfig::new("http://localhost:8080", "test-key");
        assert_eq!(config.base_url, "http://localhost:8080");
        assert_eq!(config.api_key, "test-key");
    }

    #[tokio::test]
    async fn share_experience_returns_share_response_on_success() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("POST", "/experience")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{"gene_id":"gene-abc","status":"published","published_at":"2026-05-11T00:00:00Z"}"#,
            )
            .create_async()
            .await;

        let config = ClientConfig::new(server.url(), "test-key");
        let client = ExperienceRepoClient::new(config).unwrap();
        let resp = client.share_experience(sample_envelope()).await.unwrap();

        assert_eq!(resp.gene_id, "gene-abc");
        assert_eq!(resp.status, "published");
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn share_experience_maps_http_error_to_client_error() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("POST", "/experience")
            .with_status(401)
            .with_body(r#"{"error":"unauthorized"}"#)
            .create_async()
            .await;

        let config = ClientConfig::new(server.url(), "bad-key");
        let client = ExperienceRepoClient::new(config).unwrap();
        let err = client
            .share_experience(sample_envelope())
            .await
            .unwrap_err();

        assert!(matches!(err, ClientError::HttpError(_)));
        mock.assert_async().await;
    }
}
