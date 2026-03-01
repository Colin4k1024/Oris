use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;

use crate::error::RetrieverError;
use crate::schemas::{Document, Retriever};

/// Voting strategy for ensemble retriever
#[derive(Debug, Clone)]
pub enum VotingStrategy {
    /// Weighted voting (each retriever has a weight)
    Weighted { weights: Vec<f64> },
    /// Majority voting (document must appear in majority of retrievers)
    Majority { threshold: f64 },
    /// Simple voting (count appearances)
    Simple,
}

impl Default for VotingStrategy {
    fn default() -> Self {
        Self::Weighted {
            weights: Vec::new(), // Will use equal weights if empty
        }
    }
}

/// Configuration for Ensemble retriever
#[derive(Debug, Clone)]
pub struct EnsembleRetrieverConfig {
    /// Voting strategy
    pub strategy: VotingStrategy,
    /// Maximum number of documents to return
    pub top_k: usize,
}

impl Default for EnsembleRetrieverConfig {
    fn default() -> Self {
        Self {
            strategy: VotingStrategy::default(),
            top_k: 5,
        }
    }
}

/// Ensemble retriever that uses voting mechanism from multiple retrievers
pub struct EnsembleRetriever {
    config: EnsembleRetrieverConfig,
    retrievers: Vec<Arc<dyn Retriever>>,
}

impl EnsembleRetriever {
    /// Create a new ensemble retriever
    pub fn new(retrievers: Vec<Arc<dyn Retriever>>) -> Self {
        Self::with_config(retrievers, EnsembleRetrieverConfig::default())
    }

    /// Create a new ensemble retriever with custom config
    pub fn with_config(
        retrievers: Vec<Arc<dyn Retriever>>,
        config: EnsembleRetrieverConfig,
    ) -> Self {
        Self { config, retrievers }
    }

    /// Add a retriever
    pub fn add_retriever(&mut self, retriever: Arc<dyn Retriever>) {
        self.retrievers.push(retriever);
    }

    /// Generate a unique key for a document
    fn document_key(doc: &Document) -> String {
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

    /// Vote using weighted strategy
    fn vote_weighted(&self, all_results: &[Vec<Document>], weights: &[f64]) -> Vec<Document> {
        let mut doc_votes: HashMap<String, f64> = HashMap::new();
        let mut doc_map: HashMap<String, Document> = HashMap::new();

        // Calculate weighted votes
        for (results, weight) in all_results.iter().zip(weights.iter()) {
            for doc in results {
                let doc_key = Self::document_key(doc);
                *doc_votes.entry(doc_key.clone()).or_insert(0.0) += *weight;
                doc_map.entry(doc_key).or_insert_with(|| doc.clone());
            }
        }

        // Sort by votes and return top_k
        let mut voted_docs: Vec<(String, f64)> = doc_votes.into_iter().collect();
        voted_docs.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        voted_docs
            .into_iter()
            .take(self.config.top_k)
            .filter_map(|(key, votes)| {
                doc_map.get(&key).map(|doc| {
                    let mut doc = doc.clone();
                    doc.metadata
                        .insert("ensemble_votes".to_string(), Value::from(votes));
                    doc
                })
            })
            .collect()
    }

    /// Vote using majority strategy
    fn vote_majority(&self, all_results: &[Vec<Document>], threshold: f64) -> Vec<Document> {
        let mut doc_votes: HashMap<String, usize> = HashMap::new();
        let mut doc_map: HashMap<String, Document> = HashMap::new();
        let total_retrievers = all_results.len();

        // Count votes
        for results in all_results {
            for doc in results {
                let doc_key = Self::document_key(doc);
                *doc_votes.entry(doc_key.clone()).or_insert(0) += 1;
                doc_map.entry(doc_key).or_insert_with(|| doc.clone());
            }
        }

        // Filter by threshold
        let min_votes = (total_retrievers as f64 * threshold).ceil() as usize;
        let mut voted_docs: Vec<(String, usize)> = doc_votes
            .into_iter()
            .filter(|(_, votes)| *votes >= min_votes)
            .collect();
        voted_docs.sort_by(|a, b| b.1.cmp(&a.1));

        voted_docs
            .into_iter()
            .take(self.config.top_k)
            .filter_map(|(key, votes)| {
                doc_map.get(&key).map(|doc| {
                    let mut doc = doc.clone();
                    doc.metadata
                        .insert("ensemble_votes".to_string(), Value::from(votes));
                    doc
                })
            })
            .collect()
    }

    /// Vote using simple counting
    fn vote_simple(&self, all_results: &[Vec<Document>]) -> Vec<Document> {
        let mut doc_votes: HashMap<String, usize> = HashMap::new();
        let mut doc_map: HashMap<String, Document> = HashMap::new();

        // Count votes
        for results in all_results {
            for doc in results {
                let doc_key = Self::document_key(doc);
                *doc_votes.entry(doc_key.clone()).or_insert(0) += 1;
                doc_map.entry(doc_key).or_insert_with(|| doc.clone());
            }
        }

        // Sort by votes and return top_k
        let mut voted_docs: Vec<(String, usize)> = doc_votes.into_iter().collect();
        voted_docs.sort_by(|a, b| b.1.cmp(&a.1));

        voted_docs
            .into_iter()
            .take(self.config.top_k)
            .filter_map(|(key, votes)| {
                doc_map.get(&key).map(|doc| {
                    let mut doc = doc.clone();
                    doc.metadata
                        .insert("ensemble_votes".to_string(), Value::from(votes));
                    doc
                })
            })
            .collect()
    }
}

#[async_trait]
impl Retriever for EnsembleRetriever {
    async fn get_relevant_documents(&self, query: &str) -> Result<Vec<Document>, RetrieverError> {
        // Retrieve from all retrievers
        let mut all_results = Vec::new();
        for retriever in &self.retrievers {
            match retriever.get_relevant_documents(query).await {
                Ok(results) => all_results.push(results),
                Err(_e) => {
                    log::warn!("EnsembleRetriever: One retriever failed, continuing with others");
                    all_results.push(Vec::new());
                }
            }
        }

        // Vote based on strategy
        let voted = match &self.config.strategy {
            VotingStrategy::Weighted { weights } => {
                if weights.len() == all_results.len() && !weights.is_empty() {
                    self.vote_weighted(&all_results, weights)
                } else {
                    // Default to equal weights
                    let equal_weights = vec![1.0; all_results.len()];
                    self.vote_weighted(&all_results, &equal_weights)
                }
            }
            VotingStrategy::Majority { threshold } => self.vote_majority(&all_results, *threshold),
            VotingStrategy::Simple => self.vote_simple(&all_results),
        };

        Ok(voted)
    }
}
