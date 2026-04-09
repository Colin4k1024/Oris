//! HTTP client for Experience Repository.

use reqwest::Client;
use url::Url;

pub use crate::api::response::{FetchResponse, NetworkAsset};

/// Configuration for the Experience Repository client.
#[derive(Debug, Clone)]
pub struct ClientConfig {
    pub base_url: String,
    pub api_key: String,
}

impl ClientConfig {
    /// Create a new client configuration.
    pub fn new(base_url: impl Into<String>, api_key: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            api_key: api_key.into(),
        }
    }
}

/// Client for accessing Experience Repository API.
#[derive(Debug, Clone)]
pub struct ExperienceRepoClient {
    client: Client,
    config: ClientConfig,
}

impl ExperienceRepoClient {
    /// Create a new client with configuration.
    pub fn new(config: ClientConfig) -> Self {
        Self {
            client: Client::new(),
            config,
        }
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

    #[test]
    fn test_client_config() {
        let config = ClientConfig::new("http://localhost:8080", "test-key");
        assert_eq!(config.base_url, "http://localhost:8080");
        assert_eq!(config.api_key, "test-key");
    }
}
