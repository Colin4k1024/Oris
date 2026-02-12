use std::collections::HashMap;

use async_trait::async_trait;
use reqwest::Client;
use serde_json::Value;

use crate::error::RetrieverError;
use crate::schemas::{Document, Retriever};

/// Configuration for Wikipedia retriever
#[derive(Debug, Clone)]
pub struct WikipediaRetrieverConfig {
    /// Language code (e.g., "en", "zh", "es")
    pub language: String,
    /// Maximum number of documents to load
    pub load_max_docs: usize,
    /// Whether to load all available metadata
    pub load_all_available_meta: bool,
    /// HTTP client timeout
    pub timeout: Option<std::time::Duration>,
}

impl Default for WikipediaRetrieverConfig {
    fn default() -> Self {
        Self {
            language: "en".to_string(),
            load_max_docs: 3,
            load_all_available_meta: false,
            timeout: Some(std::time::Duration::from_secs(30)),
        }
    }
}

/// Wikipedia retriever that fetches articles from Wikipedia
#[derive(Debug, Clone)]
pub struct WikipediaRetriever {
    config: WikipediaRetrieverConfig,
    client: Client,
}

impl WikipediaRetriever {
    /// Create a new Wikipedia retriever with default config
    pub fn new() -> Self {
        Self::with_config(WikipediaRetrieverConfig::default())
    }

    /// Create a new Wikipedia retriever with custom config
    pub fn with_config(config: WikipediaRetrieverConfig) -> Self {
        let mut client_builder = Client::builder();
        if let Some(timeout) = config.timeout {
            client_builder = client_builder.timeout(timeout);
        }
        let client = client_builder.build().unwrap_or_else(|_| Client::new());

        Self { config, client }
    }

    /// Set the language
    pub fn with_language<S: Into<String>>(mut self, language: S) -> Self {
        self.config.language = language.into();
        self
    }

    /// Set maximum number of documents
    pub fn with_max_docs(mut self, max_docs: usize) -> Self {
        self.config.load_max_docs = max_docs;
        self
    }

    /// Search Wikipedia for articles matching the query
    async fn search(&self, query: &str) -> Result<Vec<String>, RetrieverError> {
        let url = format!("https://{}.wikipedia.org/w/api.php", self.config.language);

        let params = [
            ("action", "query"),
            ("list", "search"),
            ("srsearch", query),
            ("format", "json"),
            ("srlimit", &self.config.load_max_docs.to_string()),
        ];

        let response = self
            .client
            .get(&url)
            .query(&params)
            .send()
            .await
            .map_err(|e| RetrieverError::WikipediaError(e.to_string()))?;

        let json: Value = response
            .json()
            .await
            .map_err(|e| RetrieverError::WikipediaError(e.to_string()))?;

        let mut titles = Vec::new();
        if let Some(query_obj) = json.get("query") {
            if let Some(search) = query_obj.get("search") {
                if let Some(search_array) = search.as_array() {
                    for item in search_array {
                        if let Some(title) = item.get("title").and_then(|t| t.as_str()) {
                            titles.push(title.to_string());
                        }
                    }
                }
            }
        }

        Ok(titles)
    }

    /// Fetch a Wikipedia page by title
    async fn fetch_page(&self, title: &str) -> Result<Document, RetrieverError> {
        let url = format!("https://{}.wikipedia.org/w/api.php", self.config.language);

        let title_encoded = urlencoding::encode(title);
        let params = [
            ("action", "query"),
            ("prop", "extracts"),
            ("exintro", "true"),
            ("explaintext", "true"),
            ("titles", &title_encoded),
            ("format", "json"),
        ];

        let response = self
            .client
            .get(&url)
            .query(&params)
            .send()
            .await
            .map_err(|e| RetrieverError::WikipediaError(e.to_string()))?;

        let json: Value = response
            .json()
            .await
            .map_err(|e| RetrieverError::WikipediaError(e.to_string()))?;

        let mut content = String::new();
        let mut page_id = None;

        if let Some(query_obj) = json.get("query") {
            if let Some(pages) = query_obj.get("pages") {
                if let Some(pages_obj) = pages.as_object() {
                    for (id, page) in pages_obj {
                        page_id = Some(id.clone());
                        if let Some(extract) = page.get("extract").and_then(|e| e.as_str()) {
                            content = extract.to_string();
                        }
                        if let Some(full_title) = page.get("title").and_then(|t| t.as_str()) {
                            if content.is_empty() {
                                // Try to get full content if extract is empty
                                let full_url = format!(
                                    "https://{}.wikipedia.org/w/api.php",
                                    self.config.language
                                );
                                let full_params = [
                                    ("action", "query"),
                                    ("prop", "extracts"),
                                    ("explaintext", "true"),
                                    ("titles", full_title),
                                    ("format", "json"),
                                ];
                                if let Ok(full_response) =
                                    self.client.get(&full_url).query(&full_params).send().await
                                {
                                    if let Ok(full_json) = full_response.json::<Value>().await {
                                        if let Some(full_query) = full_json.get("query") {
                                            if let Some(full_pages) = full_query.get("pages") {
                                                if let Some(full_pages_obj) = full_pages.as_object()
                                                {
                                                    for (_, full_page) in full_pages_obj {
                                                        if let Some(full_extract) = full_page
                                                            .get("extract")
                                                            .and_then(|e| e.as_str())
                                                        {
                                                            content = full_extract.to_string();
                                                        }
                                                        break;
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        break;
                    }
                }
            }
        }

        let mut metadata = HashMap::new();
        metadata.insert("source".to_string(), Value::from("wikipedia"));
        metadata.insert(
            "language".to_string(),
            Value::from(self.config.language.clone()),
        );
        if let Some(id) = page_id {
            metadata.insert("page_id".to_string(), Value::from(id));
        }
        metadata.insert("title".to_string(), Value::from(title));

        Ok(Document::new(content).with_metadata(metadata))
    }
}

impl Default for WikipediaRetriever {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Retriever for WikipediaRetriever {
    async fn get_relevant_documents(&self, query: &str) -> Result<Vec<Document>, RetrieverError> {
        // First, search for relevant articles
        let titles = self.search(query).await?;

        if titles.is_empty() {
            return Ok(vec![]);
        }

        // Fetch each article
        let mut documents = Vec::new();
        for title in titles {
            let doc = self.fetch_page(&title).await?;
            documents.push(doc);
        }

        Ok(documents)
    }
}
