use std::collections::HashMap;

use async_trait::async_trait;

use crate::error::RetrieverError;
use reqwest::Client;
use serde_json::{json, Value};

use crate::schemas::{Document, Retriever};

/// Configuration for Tavily Search API retriever
#[derive(Debug, Clone)]
pub struct TavilyRetrieverConfig {
    /// Tavily API key
    pub api_key: String,
    /// Maximum number of results
    pub max_results: usize,
    /// Include answer in response
    pub include_answer: bool,
    /// Include raw content
    pub include_raw_content: bool,
    /// HTTP client timeout
    pub timeout: Option<std::time::Duration>,
}

/// Tavily Search API retriever for real-time web search
#[derive(Debug, Clone)]
pub struct TavilySearchAPIRetriever {
    config: TavilyRetrieverConfig,
    client: Client,
}

impl TavilySearchAPIRetriever {
    /// Create a new Tavily retriever
    pub fn new<S: Into<String>>(api_key: S) -> Self {
        Self::with_config(TavilyRetrieverConfig {
            api_key: api_key.into(),
            max_results: 5,
            include_answer: false,
            include_raw_content: false,
            timeout: Some(std::time::Duration::from_secs(30)),
        })
    }

    /// Create a new Tavily retriever with custom config
    pub fn with_config(config: TavilyRetrieverConfig) -> Self {
        let mut client_builder = Client::builder();
        if let Some(timeout) = config.timeout {
            client_builder = client_builder.timeout(timeout);
        }
        let client = client_builder.build().unwrap_or_else(|_| Client::new());

        Self { config, client }
    }

    /// Set maximum number of results
    pub fn with_max_results(mut self, max_results: usize) -> Self {
        self.config.max_results = max_results;
        self
    }

    /// Set whether to include answer
    pub fn with_include_answer(mut self, include: bool) -> Self {
        self.config.include_answer = include;
        self
    }

    /// Set whether to include raw content
    pub fn with_include_raw_content(mut self, include: bool) -> Self {
        self.config.include_raw_content = include;
        self
    }

    /// Search using Tavily API
    async fn search(&self, query: &str) -> Result<Vec<Document>, RetrieverError> {
        let url = "https://api.tavily.com/search";

        let request_body = json!({
            "api_key": self.config.api_key,
            "query": query,
            "max_results": self.config.max_results,
            "include_answer": self.config.include_answer,
            "include_raw_content": self.config.include_raw_content,
        });

        let response = self
            .client
            .post(url)
            .json(&request_body)
            .send()
            .await
            .map_err(|e| RetrieverError::TavilyError(e.to_string()))?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(RetrieverError::TavilyError(format!(
                "{}: {}",
                status, error_text
            )));
        }

        let json: Value = response
            .json()
            .await
            .map_err(|e| RetrieverError::TavilyError(e.to_string()))?;

        let mut documents = Vec::new();

        // Handle answer if present
        if let Some(answer) = json.get("answer") {
            if let Some(answer_str) = answer.as_str() {
                let mut metadata = HashMap::new();
                metadata.insert("source".to_string(), Value::from("tavily"));
                metadata.insert("type".to_string(), Value::from("answer"));
                documents.push(Document::new(answer_str.to_string()).with_metadata(metadata));
            }
        }

        // Handle results
        if let Some(results) = json.get("results").and_then(|r| r.as_array()) {
            for result in results {
                let mut content = String::new();
                let mut metadata = HashMap::new();
                metadata.insert("source".to_string(), Value::from("tavily"));
                metadata.insert("type".to_string(), Value::from("result"));

                if let Some(title) = result.get("title").and_then(|t| t.as_str()) {
                    content.push_str(&format!("Title: {}\n\n", title));
                    metadata.insert("title".to_string(), Value::from(title));
                }

                if let Some(url) = result.get("url").and_then(|u| u.as_str()) {
                    metadata.insert("url".to_string(), Value::from(url));
                }

                if let Some(content_str) = result.get("content").and_then(|c| c.as_str()) {
                    content.push_str(content_str);
                } else if let Some(raw_content) = result.get("raw_content").and_then(|c| c.as_str())
                {
                    content.push_str(raw_content);
                }

                if let Some(score) = result.get("score").and_then(|s| s.as_f64()) {
                    metadata.insert("score".to_string(), Value::from(score));
                }

                if !content.trim().is_empty() {
                    documents.push(Document::new(content).with_metadata(metadata));
                }
            }
        }

        Ok(documents)
    }
}

#[async_trait]
impl Retriever for TavilySearchAPIRetriever {
    async fn get_relevant_documents(&self, query: &str) -> Result<Vec<Document>, RetrieverError> {
        self.search(query).await
    }
}
