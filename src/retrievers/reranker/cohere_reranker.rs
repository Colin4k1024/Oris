use std::error::Error;
use std::sync::Arc;

use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};

use crate::error::RetrieverError;
use crate::schemas::{Document, Retriever};

/// Configuration for Cohere reranker
#[derive(Debug, Clone)]
pub struct CohereRerankerConfig {
    /// Cohere API key
    pub api_key: String,
    /// Model to use for reranking
    pub model: String,
    /// Top K documents to return after reranking
    pub top_k: Option<usize>,
    /// HTTP client timeout
    pub timeout: Option<std::time::Duration>,
}

impl CohereRerankerConfig {
    /// Create a new config with API key
    pub fn new(api_key: String) -> Self {
        Self {
            api_key,
            model: "rerank-english-v3.0".to_string(),
            top_k: None,
            timeout: Some(std::time::Duration::from_secs(30)),
        }
    }
}

#[derive(Serialize)]
struct RerankRequest {
    model: String,
    query: String,
    documents: Vec<String>,
    top_n: Option<usize>,
}

#[derive(Deserialize)]
struct RerankResponse {
    results: Vec<RerankResult>,
}

#[derive(Deserialize)]
struct RerankResult {
    index: usize,
    relevance_score: f64,
}

/// Cohere reranker that uses Cohere API to rerank documents
pub struct CohereReranker {
    base_retriever: Arc<dyn Retriever>,
    config: CohereRerankerConfig,
    client: Client,
}

impl CohereReranker {
    /// Create a new Cohere reranker
    pub fn new(base_retriever: Arc<dyn Retriever>, api_key: String) -> Self {
        Self::with_config(base_retriever, CohereRerankerConfig::new(api_key))
    }

    /// Create a new Cohere reranker with custom config
    pub fn with_config(base_retriever: Arc<dyn Retriever>, config: CohereRerankerConfig) -> Self {
        let mut client_builder = Client::builder();
        if let Some(timeout) = config.timeout {
            client_builder = client_builder.timeout(timeout);
        }
        let client = client_builder.build().unwrap_or_else(|_| Client::new());

        Self {
            base_retriever,
            config,
            client,
        }
    }

    /// Rerank documents using Cohere API
    async fn rerank(
        &self,
        query: &str,
        documents: Vec<Document>,
    ) -> Result<Vec<Document>, Box<dyn Error>> {
        if documents.is_empty() {
            return Ok(documents);
        }

        let texts: Vec<String> = documents.iter().map(|d| d.page_content.clone()).collect();

        let request = RerankRequest {
            model: self.config.model.clone(),
            query: query.to_string(),
            documents: texts.clone(),
            top_n: self.config.top_k,
        };

        let response = self
            .client
            .post("https://api.cohere.ai/v1/rerank")
            .header("Authorization", format!("Bearer {}", self.config.api_key))
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await?;

        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(format!("Cohere API error: {}", error_text).into());
        }

        let rerank_response: RerankResponse = response.json().await?;

        // Sort results by relevance score (descending) and map back to documents
        let mut indexed_results: Vec<(usize, f64)> = rerank_response
            .results
            .into_iter()
            .map(|r| (r.index, r.relevance_score))
            .collect();
        indexed_results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        let reranked: Vec<Document> = indexed_results
            .into_iter()
            .filter_map(|(idx, _)| documents.get(idx).cloned())
            .collect();

        Ok(reranked)
    }
}

#[async_trait]
impl Retriever for CohereReranker {
    async fn get_relevant_documents(&self, query: &str) -> Result<Vec<Document>, RetrieverError> {
        // Get documents from base retriever
        let documents = self.base_retriever.get_relevant_documents(query).await?;

        // Rerank documents
        self.rerank(query, documents)
            .await
            .map_err(|e| RetrieverError::RerankerError(e.to_string()))
    }
}
