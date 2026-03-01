use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;

use crate::error::RetrieverError;
use crate::schemas::{Document, Retriever};

/// Merge strategy for combining results from multiple retrievers
#[derive(Debug, Clone)]
pub enum MergeStrategy {
    /// Reciprocal Rank Fusion (RRF)
    ReciprocalRankFusion { k: f64 },
    /// Weighted average of scores
    Weighted { weights: Vec<f64> },
    /// Simple concatenation with deduplication
    Concatenate,
}

impl Default for MergeStrategy {
    fn default() -> Self {
        Self::ReciprocalRankFusion { k: 60.0 }
    }
}

/// Configuration for Merger retriever
#[derive(Debug, Clone)]
pub struct MergerRetrieverConfig {
    /// Merge strategy
    pub strategy: MergeStrategy,
    /// Maximum number of documents to return
    pub top_k: usize,
}

impl Default for MergerRetrieverConfig {
    fn default() -> Self {
        Self {
            strategy: MergeStrategy::default(),
            top_k: 5,
        }
    }
}

/// Merger retriever that combines results from multiple retrievers
pub struct MergerRetriever {
    pub config: MergerRetrieverConfig,
    retrievers: Vec<Arc<dyn Retriever>>,
}

impl MergerRetriever {
    /// Create a new merger retriever
    pub fn new(retrievers: Vec<Arc<dyn Retriever>>) -> Self {
        Self::with_config(retrievers, MergerRetrieverConfig::default())
    }

    /// Create a new merger retriever with custom config
    pub fn with_config(retrievers: Vec<Arc<dyn Retriever>>, config: MergerRetrieverConfig) -> Self {
        Self { config, retrievers }
    }

    /// Add a retriever
    pub fn add_retriever(&mut self, retriever: Arc<dyn Retriever>) {
        self.retrievers.push(retriever);
    }

    /// Merge documents using Reciprocal Rank Fusion
    fn merge_rrf(&self, all_results: &[Vec<Document>], k: f64) -> Vec<Document> {
        let mut doc_scores: HashMap<String, f64> = HashMap::new();
        let mut doc_map: HashMap<String, Document> = HashMap::new();

        // Calculate RRF scores
        for results in all_results {
            for (rank, doc) in results.iter().enumerate() {
                let score = 1.0 / (k + rank as f64 + 1.0);
                let doc_key = Self::document_key(doc);
                *doc_scores.entry(doc_key.clone()).or_insert(0.0) += score;
                doc_map.entry(doc_key).or_insert_with(|| doc.clone());
            }
        }

        // Sort by score and return top_k
        let mut scored_docs: Vec<(String, f64)> = doc_scores.into_iter().collect();
        scored_docs.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        scored_docs
            .into_iter()
            .take(self.config.top_k)
            .filter_map(|(key, score)| {
                doc_map.get(&key).map(|doc| {
                    let mut doc = doc.clone();
                    doc.metadata
                        .insert("rrf_score".to_string(), Value::from(score));
                    doc
                })
            })
            .collect()
    }

    /// Merge documents using weighted scores
    fn merge_weighted(&self, all_results: &[Vec<Document>], weights: &[f64]) -> Vec<Document> {
        let mut doc_scores: HashMap<String, f64> = HashMap::new();
        let mut doc_map: HashMap<String, Document> = HashMap::new();

        // Calculate weighted scores
        for (results, weight) in all_results.iter().zip(weights.iter()) {
            for (rank, doc) in results.iter().enumerate() {
                let score = *weight / (rank as f64 + 1.0);
                let doc_key = Self::document_key(doc);
                *doc_scores.entry(doc_key.clone()).or_insert(0.0) += score;
                doc_map.entry(doc_key).or_insert_with(|| doc.clone());
            }
        }

        // Sort by score and return top_k
        let mut scored_docs: Vec<(String, f64)> = doc_scores.into_iter().collect();
        scored_docs.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        scored_docs
            .into_iter()
            .take(self.config.top_k)
            .filter_map(|(key, score)| {
                doc_map.get(&key).map(|doc| {
                    let mut doc = doc.clone();
                    doc.metadata
                        .insert("weighted_score".to_string(), Value::from(score));
                    doc
                })
            })
            .collect()
    }

    /// Merge documents by concatenation with deduplication
    fn merge_concatenate(&self, all_results: &[Vec<Document>]) -> Vec<Document> {
        let mut seen = std::collections::HashSet::new();
        let mut merged = Vec::new();

        for results in all_results {
            for doc in results {
                let key = Self::document_key(doc);
                if !seen.contains(&key) {
                    seen.insert(key);
                    merged.push(doc.clone());
                    if merged.len() >= self.config.top_k {
                        return merged;
                    }
                }
            }
        }

        merged
    }

    /// Generate a unique key for a document (for deduplication)
    fn document_key(doc: &Document) -> String {
        // Use content hash as key, or combine source + content
        let preview = &doc.page_content[..doc.page_content.len().min(100)];
        if !doc.metadata.is_empty() {
            if let Some(source) = doc.metadata.get("source").and_then(|s| s.as_str()) {
                format!("{}:{}", source, preview)
            } else {
                preview.to_string()
            }
        } else {
            preview.to_string()
        }
    }
}

#[async_trait]
impl Retriever for MergerRetriever {
    async fn get_relevant_documents(&self, query: &str) -> Result<Vec<Document>, RetrieverError> {
        // Retrieve from all retrievers
        let mut all_results = Vec::new();
        for retriever in &self.retrievers {
            match retriever.get_relevant_documents(query).await {
                Ok(results) => all_results.push(results),
                Err(_e) => {
                    log::warn!("MergerRetriever: One retriever failed, continuing with others");
                    all_results.push(Vec::new());
                }
            }
        }

        // Merge results based on strategy
        let merged = match &self.config.strategy {
            MergeStrategy::ReciprocalRankFusion { k } => self.merge_rrf(&all_results, *k),
            MergeStrategy::Weighted { weights } => {
                if weights.len() == all_results.len() {
                    self.merge_weighted(&all_results, weights)
                } else {
                    // Default to equal weights if mismatch
                    let equal_weights = vec![1.0; all_results.len()];
                    self.merge_weighted(&all_results, &equal_weights)
                }
            }
            MergeStrategy::Concatenate => self.merge_concatenate(&all_results),
        };

        Ok(merged)
    }
}
