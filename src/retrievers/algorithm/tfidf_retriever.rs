use std::collections::HashMap;

use async_trait::async_trait;
use serde_json::Value;

use crate::error::RetrieverError;
use crate::schemas::{Document, Retriever};

/// Configuration for TF-IDF retriever
#[derive(Debug, Clone)]
pub struct TFIDFRetrieverConfig {
    /// Maximum number of documents to return
    pub top_k: usize,
}

impl Default for TFIDFRetrieverConfig {
    fn default() -> Self {
        Self { top_k: 5 }
    }
}

/// TF-IDF retriever using Term Frequency-Inverse Document Frequency
#[derive(Debug)]
pub struct TFIDFRetriever {
    config: TFIDFRetrieverConfig,
    documents: Vec<Document>,
    // TF-IDF vectors for each document
    tfidf_vectors: Vec<HashMap<String, f64>>,
    // Vocabulary (all unique terms)
    vocabulary: Vec<String>,
    // IDF values for each term
    idf_values: HashMap<String, f64>,
}

impl TFIDFRetriever {
    /// Create a new TF-IDF retriever with documents
    pub fn new(documents: Vec<Document>) -> Self {
        Self::with_config(documents, TFIDFRetrieverConfig::default())
    }

    /// Create a new TF-IDF retriever with custom config
    pub fn with_config(documents: Vec<Document>, config: TFIDFRetrieverConfig) -> Self {
        let mut retriever = Self {
            config,
            documents,
            tfidf_vectors: Vec::new(),
            vocabulary: Vec::new(),
            idf_values: HashMap::new(),
        };
        retriever.build_tfidf();
        retriever
    }

    /// Build TF-IDF vectors
    fn build_tfidf(&mut self) {
        let total_docs = self.documents.len() as f64;
        let mut term_doc_counts: HashMap<String, usize> = HashMap::new();
        let mut doc_term_counts: Vec<HashMap<String, usize>> = Vec::new();

        // First pass: count term frequencies and document frequencies
        for doc in &self.documents {
            let tokens = Self::tokenize(&doc.page_content);
            let mut term_counts = HashMap::new();

            for token in &tokens {
                *term_counts.entry(token.clone()).or_insert(0) += 1;
                *term_doc_counts.entry(token.clone()).or_insert(0) += 1;
            }

            doc_term_counts.push(term_counts);
        }

        // Build vocabulary and calculate IDF
        self.vocabulary = term_doc_counts.keys().cloned().collect();
        for (term, doc_count) in &term_doc_counts {
            let idf = (total_docs / (*doc_count as f64)).ln();
            self.idf_values.insert(term.clone(), idf);
        }

        // Second pass: calculate TF-IDF vectors
        self.tfidf_vectors.clear();
        for term_counts in &doc_term_counts {
            let total_terms: usize = term_counts.values().sum();
            let mut tfidf_vector = HashMap::new();

            for (term, count) in term_counts {
                let tf = *count as f64 / total_terms as f64;
                let idf = self.idf_values.get(term).copied().unwrap_or(0.0);
                tfidf_vector.insert(term.clone(), tf * idf);
            }

            self.tfidf_vectors.push(tfidf_vector);
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

    /// Calculate cosine similarity between two vectors
    fn cosine_similarity(vec1: &HashMap<String, f64>, vec2: &HashMap<String, f64>) -> f64 {
        let mut dot_product = 0.0;
        let mut norm1 = 0.0;
        let mut norm2 = 0.0;

        // Calculate dot product and norms
        let all_terms: Vec<&String> = vec1.keys().chain(vec2.keys()).collect();
        let unique_terms: Vec<&String> = all_terms
            .into_iter()
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();

        for term in unique_terms {
            let val1 = vec1.get(term).copied().unwrap_or(0.0);
            let val2 = vec2.get(term).copied().unwrap_or(0.0);
            dot_product += val1 * val2;
            norm1 += val1 * val1;
            norm2 += val2 * val2;
        }

        if norm1 == 0.0 || norm2 == 0.0 {
            return 0.0;
        }

        dot_product / (norm1.sqrt() * norm2.sqrt())
    }

    /// Add documents to the index
    pub fn add_documents(&mut self, documents: Vec<Document>) {
        self.documents.extend(documents);
        self.build_tfidf();
    }
}

#[async_trait]
impl Retriever for TFIDFRetriever {
    async fn get_relevant_documents(&self, query: &str) -> Result<Vec<Document>, RetrieverError> {
        // Build query TF-IDF vector
        let query_tokens = Self::tokenize(query);
        let mut query_term_counts: HashMap<String, usize> = HashMap::new();
        for token in &query_tokens {
            *query_term_counts.entry(token.clone()).or_insert(0) += 1;
        }

        let total_query_terms: usize = query_term_counts.values().sum();
        let mut query_vector = HashMap::new();
        for (term, count) in &query_term_counts {
            let tf = *count as f64 / total_query_terms as f64;
            let idf = self.idf_values.get(term).copied().unwrap_or(0.0);
            query_vector.insert(term.clone(), tf * idf);
        }

        // Calculate cosine similarity with each document
        let mut scored_docs: Vec<(usize, f64)> = self
            .tfidf_vectors
            .iter()
            .enumerate()
            .map(|(idx, doc_vector)| {
                let similarity = Self::cosine_similarity(&query_vector, doc_vector);
                (idx, similarity)
            })
            .collect();

        // Sort by similarity (descending)
        scored_docs.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        // Return top_k documents
        let top_k = self.config.top_k.min(scored_docs.len());
        let mut results = Vec::new();
        for (doc_id, similarity) in scored_docs.into_iter().take(top_k) {
            if let Some(doc) = self.documents.get(doc_id) {
                let mut doc = doc.clone();
                // Add similarity score to metadata
                doc.metadata
                    .insert("tfidf_similarity".to_string(), Value::from(similarity));
                results.push(doc);
            }
        }

        Ok(results)
    }
}
