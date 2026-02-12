use std::collections::HashMap;

use async_trait::async_trait;
use reqwest::Client;
use serde_json::Value;

use crate::error::RetrieverError;
use crate::schemas::{Document, Retriever};

/// Configuration for arXiv retriever
#[derive(Debug, Clone)]
pub struct ArxivRetrieverConfig {
    /// Maximum number of documents to retrieve
    pub max_docs: usize,
    /// HTTP client timeout
    pub timeout: Option<std::time::Duration>,
}

impl Default for ArxivRetrieverConfig {
    fn default() -> Self {
        Self {
            max_docs: 3,
            timeout: Some(std::time::Duration::from_secs(30)),
        }
    }
}

/// arXiv retriever that fetches academic papers from arXiv
#[derive(Debug, Clone)]
pub struct ArxivRetriever {
    config: ArxivRetrieverConfig,
    client: Client,
}

impl ArxivRetriever {
    /// Create a new arXiv retriever with default config
    pub fn new() -> Self {
        Self::with_config(ArxivRetrieverConfig::default())
    }

    /// Create a new arXiv retriever with custom config
    pub fn with_config(config: ArxivRetrieverConfig) -> Self {
        let mut client_builder = Client::builder();
        if let Some(timeout) = config.timeout {
            client_builder = client_builder.timeout(timeout);
        }
        let client = client_builder.build().unwrap_or_else(|_| Client::new());

        Self { config, client }
    }

    /// Set maximum number of documents
    pub fn with_max_docs(mut self, max_docs: usize) -> Self {
        self.config.max_docs = max_docs;
        self
    }

    /// Search arXiv for papers matching the query
    async fn search_arxiv(&self, query: &str) -> Result<Vec<Document>, RetrieverError> {
        // arXiv API endpoint
        let url = "http://export.arxiv.org/api/query";

        let params = [
            ("search_query", query),
            ("start", "0"),
            ("max_results", &self.config.max_docs.to_string()),
            ("sortBy", "relevance"),
            ("sortOrder", "descending"),
        ];

        let response = self
            .client
            .get(url)
            .query(&params)
            .send()
            .await
            .map_err(|e| RetrieverError::ArxivError(e.to_string()))?;
        let xml_content = response
            .text()
            .await
            .map_err(|e| RetrieverError::ArxivError(e.to_string()))?;

        // Parse XML response
        let mut documents = Vec::new();
        let mut current_entry = HashMap::new();
        let mut current_text = String::new();
        let mut in_entry = false;
        let mut in_title = false;
        let mut in_summary = false;
        let mut in_author = false;
        let _current_tag = String::new();

        for line in xml_content.lines() {
            let line = line.trim();
            if line.starts_with("<entry>") {
                in_entry = true;
                current_entry.clear();
                current_text.clear();
            } else if line.starts_with("</entry>") {
                if in_entry {
                    // Create document from entry
                    let title = current_entry
                        .get("title")
                        .cloned()
                        .unwrap_or_else(|| "Untitled".to_string());
                    let summary = current_entry
                        .get("summary")
                        .cloned()
                        .unwrap_or_else(|| String::new());
                    let authors = current_entry
                        .get("authors")
                        .cloned()
                        .unwrap_or_else(|| String::new());
                    let id = current_entry
                        .get("id")
                        .cloned()
                        .unwrap_or_else(|| String::new());

                    let content = format!(
                        "Title: {}\n\nAuthors: {}\n\nAbstract: {}",
                        title, authors, summary
                    );

                    let mut metadata = HashMap::new();
                    metadata.insert("source".to_string(), Value::from("arxiv"));
                    metadata.insert("title".to_string(), Value::from(title));
                    metadata.insert("id".to_string(), Value::from(id));
                    if !authors.is_empty() {
                        metadata.insert("authors".to_string(), Value::from(authors));
                    }

                    documents.push(Document::new(content).with_metadata(metadata));
                }
                in_entry = false;
                in_title = false;
                in_summary = false;
                in_author = false;
            } else if line.starts_with("<title>") {
                in_title = true;
                current_text.clear();
            } else if line.starts_with("</title>") {
                if in_entry && in_title {
                    current_entry.insert("title".to_string(), current_text.trim().to_string());
                }
                in_title = false;
                current_text.clear();
            } else if line.starts_with("<summary>") {
                in_summary = true;
                current_text.clear();
            } else if line.starts_with("</summary>") {
                if in_entry && in_summary {
                    current_entry.insert("summary".to_string(), current_text.trim().to_string());
                }
                in_summary = false;
                current_text.clear();
            } else if line.starts_with("<id>") {
                let id_start = line.find('>').unwrap_or(0) + 1;
                let id_end = line.find('<').unwrap_or(line.len());
                if id_start < id_end {
                    let id = line[id_start..id_end].trim();
                    if in_entry && id.starts_with("http://arxiv.org/abs/") {
                        current_entry.insert("id".to_string(), id.to_string());
                    }
                }
            } else if line.starts_with("<name>") {
                in_author = true;
                current_text.clear();
            } else if line.starts_with("</name>") {
                if in_entry && in_author {
                    let author_name = current_text.trim().to_string();
                    if let Some(existing) = current_entry.get_mut("authors") {
                        *existing = format!("{}, {}", existing, author_name);
                    } else {
                        current_entry.insert("authors".to_string(), author_name);
                    }
                }
                in_author = false;
                current_text.clear();
            } else if in_title || in_summary || in_author {
                current_text.push_str(line);
                current_text.push(' ');
            }
        }

        Ok(documents)
    }
}

impl Default for ArxivRetriever {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Retriever for ArxivRetriever {
    async fn get_relevant_documents(&self, query: &str) -> Result<Vec<Document>, RetrieverError> {
        self.search_arxiv(query).await
    }
}
