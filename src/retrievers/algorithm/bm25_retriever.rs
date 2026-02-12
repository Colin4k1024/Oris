use std::collections::{HashMap, HashSet};

use async_trait::async_trait;
use serde_json::Value;

use crate::error::RetrieverError;
use crate::schemas::{Document, Retriever};

/// BM25 parameters
#[derive(Debug, Clone)]
pub struct BM25Params {
    /// k1 parameter (term frequency saturation)
    pub k1: f64,
    /// b parameter (length normalization)
    pub b: f64,
}

impl Default for BM25Params {
    fn default() -> Self {
        Self { k1: 1.5, b: 0.75 }
    }
}

/// Configuration for BM25 retriever
#[derive(Debug, Clone)]
pub struct BM25RetrieverConfig {
    /// BM25 parameters
    pub params: BM25Params,
    /// Maximum number of documents to return
    pub top_k: usize,
}

impl Default for BM25RetrieverConfig {
    fn default() -> Self {
        Self {
            params: BM25Params::default(),
            top_k: 5,
        }
    }
}

/// BM25 retriever using the BM25 ranking algorithm
#[derive(Debug)]
pub struct BM25Retriever {
    config: BM25RetrieverConfig,
    documents: Vec<Document>,
    // Inverted index: term -> (document_id, term_frequency)
    inverted_index: HashMap<String, Vec<(usize, usize)>>,
    // Document lengths
    doc_lengths: Vec<usize>,
    // Average document length
    avg_doc_length: f64,
    // Document frequencies: term -> number of documents containing it
    doc_frequencies: HashMap<String, usize>,
    // Total number of documents
    total_docs: usize,
}

impl BM25Retriever {
    /// Create a new BM25 retriever with documents
    pub fn new(documents: Vec<Document>) -> Self {
        Self::with_config(documents, BM25RetrieverConfig::default())
    }

    /// Create a new BM25 retriever with custom config
    pub fn with_config(documents: Vec<Document>, config: BM25RetrieverConfig) -> Self {
        let mut retriever = Self {
            config,
            documents,
            inverted_index: HashMap::new(),
            doc_lengths: Vec::new(),
            avg_doc_length: 0.0,
            doc_frequencies: HashMap::new(),
            total_docs: 0,
        };
        retriever.build_index();
        retriever
    }

    /// Build the inverted index from documents
    fn build_index(&mut self) {
        self.total_docs = self.documents.len();
        self.doc_lengths = vec![0; self.total_docs];
        self.inverted_index.clear();
        self.doc_frequencies.clear();

        // Simple tokenization (split by whitespace and punctuation)
        for (doc_id, doc) in self.documents.iter().enumerate() {
            let tokens = Self::tokenize(&doc.page_content);
            let mut term_counts = HashMap::new();

            for token in &tokens {
                *term_counts.entry(token.clone()).or_insert(0) += 1;
                self.doc_lengths[doc_id] += 1;
            }

            for (term, count) in term_counts {
                self.inverted_index
                    .entry(term.clone())
                    .or_insert_with(Vec::new)
                    .push((doc_id, count));
            }
        }

        // Calculate document frequencies
        for (term, postings) in &self.inverted_index {
            let unique_docs: HashSet<usize> = postings.iter().map(|(doc_id, _)| *doc_id).collect();
            self.doc_frequencies.insert(term.clone(), unique_docs.len());
        }

        // Calculate average document length
        if self.total_docs > 0 {
            let total_length: usize = self.doc_lengths.iter().sum();
            self.avg_doc_length = total_length as f64 / self.total_docs as f64;
        }
    }

    /// Simple tokenization
    fn tokenize(text: &str) -> Vec<String> {
        text.to_lowercase()
            .split(|c: char| !c.is_alphanumeric())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .collect()
    }

    /// Calculate BM25 score for a query term and document
    fn bm25_score(&self, term: &str, doc_id: usize, term_freq: usize) -> f64 {
        let df = self.doc_frequencies.get(term).copied().unwrap_or(0) as f64;
        if df == 0.0 {
            return 0.0;
        }

        let idf = ((self.total_docs as f64 - df + 0.5) / (df + 0.5)).ln();
        let doc_length = self.doc_lengths[doc_id] as f64;
        let tf = term_freq as f64;

        let numerator = idf * tf * (self.config.params.k1 + 1.0);
        let denominator = tf
            + self.config.params.k1
                * (1.0 - self.config.params.b
                    + self.config.params.b * doc_length / self.avg_doc_length);

        numerator / denominator
    }

    /// Add documents to the index
    pub fn add_documents(&mut self, documents: Vec<Document>) {
        let _start_id = self.documents.len();
        self.documents.extend(documents);
        self.build_index();
    }
}

#[async_trait]
impl Retriever for BM25Retriever {
    async fn get_relevant_documents(&self, query: &str) -> Result<Vec<Document>, RetrieverError> {
        let query_tokens = Self::tokenize(query);
        let mut doc_scores: HashMap<usize, f64> = HashMap::new();

        // Calculate BM25 scores for each document
        for term in &query_tokens {
            if let Some(postings) = self.inverted_index.get(term) {
                for (doc_id, term_freq) in postings {
                    let score = self.bm25_score(term, *doc_id, *term_freq);
                    *doc_scores.entry(*doc_id).or_insert(0.0) += score;
                }
            }
        }

        // Sort documents by score
        let mut scored_docs: Vec<(usize, f64)> = doc_scores.into_iter().collect();
        scored_docs.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        // Return top_k documents
        let top_k = self.config.top_k.min(scored_docs.len());
        let mut results = Vec::new();
        for (doc_id, _score) in scored_docs.into_iter().take(top_k) {
            if let Some(doc) = self.documents.get(doc_id) {
                let mut doc = doc.clone();
                // Add score to metadata
                doc.metadata
                    .insert("bm25_score".to_string(), Value::from(_score));
                results.push(doc);
            }
        }

        Ok(results)
    }
}
