use std::collections::HashMap;

use async_trait::async_trait;
use serde_json::Value;

use crate::error::RetrieverError;
use crate::schemas::{Document, Retriever};

/// Configuration for SVM retriever
#[derive(Debug, Clone)]
pub struct SVMRetrieverConfig {
    /// Maximum number of documents to return
    pub top_k: usize,
}

impl Default for SVMRetrieverConfig {
    fn default() -> Self {
        Self { top_k: 5 }
    }
}

/// SVM retriever using Support Vector Machine for document ranking
///
/// Note: This is a simplified implementation. For production use,
/// consider using a proper ML library like `linfa` or `libsvm`.
#[derive(Debug)]
pub struct SVMRetriever {
    config: SVMRetrieverConfig,
    documents: Vec<Document>,
    // Feature vectors for each document (simplified: TF-IDF-like features)
    feature_vectors: Vec<HashMap<String, f64>>,
    // Vocabulary
    vocabulary: Vec<String>,
}

impl SVMRetriever {
    /// Create a new SVM retriever with documents
    ///
    /// Note: This implementation uses a simplified linear classifier.
    /// For true SVM, you would need to train a model with labeled data.
    pub fn new(documents: Vec<Document>) -> Self {
        Self::with_config(documents, SVMRetrieverConfig::default())
    }

    /// Create a new SVM retriever with custom config
    pub fn with_config(documents: Vec<Document>, config: SVMRetrieverConfig) -> Self {
        let mut retriever = Self {
            config,
            documents,
            feature_vectors: Vec::new(),
            vocabulary: Vec::new(),
        };
        retriever.build_features();
        retriever
    }

    /// Build feature vectors (simplified TF-IDF features)
    fn build_features(&mut self) {
        let mut term_doc_counts: HashMap<String, usize> = HashMap::new();
        let mut doc_term_counts: Vec<HashMap<String, usize>> = Vec::new();
        let total_docs = self.documents.len() as f64;

        // First pass: count term frequencies
        for doc in &self.documents {
            let tokens = Self::tokenize(&doc.page_content);
            let mut term_counts = HashMap::new();

            for token in &tokens {
                *term_counts.entry(token.clone()).or_insert(0) += 1;
                *term_doc_counts.entry(token.clone()).or_insert(0) += 1;
            }

            doc_term_counts.push(term_counts);
        }

        // Build vocabulary
        self.vocabulary = term_doc_counts.keys().cloned().collect();

        // Calculate IDF values
        let mut idf_values: HashMap<String, f64> = HashMap::new();
        for (term, doc_count) in &term_doc_counts {
            let idf = (total_docs / (*doc_count as f64)).ln();
            idf_values.insert(term.clone(), idf);
        }

        // Build TF-IDF feature vectors
        self.feature_vectors.clear();
        for term_counts in &doc_term_counts {
            let total_terms: usize = term_counts.values().sum();
            let mut feature_vector = HashMap::new();

            for (term, count) in term_counts {
                let tf = *count as f64 / total_terms as f64;
                let idf = idf_values.get(term).copied().unwrap_or(0.0);
                feature_vector.insert(term.clone(), tf * idf);
            }

            self.feature_vectors.push(feature_vector);
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

    /// Calculate dot product (simplified linear classifier score)
    fn linear_score(query_vector: &HashMap<String, f64>, doc_vector: &HashMap<String, f64>) -> f64 {
        let mut score = 0.0;
        for (term, query_val) in query_vector {
            if let Some(doc_val) = doc_vector.get(term) {
                score += query_val * doc_val;
            }
        }
        score
    }

    /// Add documents to the index
    pub fn add_documents(&mut self, documents: Vec<Document>) {
        self.documents.extend(documents);
        self.build_features();
    }
}

#[async_trait]
impl Retriever for SVMRetriever {
    async fn get_relevant_documents(&self, query: &str) -> Result<Vec<Document>, RetrieverError> {
        // Build query feature vector (simplified)
        let query_tokens = Self::tokenize(query);
        let mut query_term_counts: HashMap<String, usize> = HashMap::new();
        for token in &query_tokens {
            *query_term_counts.entry(token.clone()).or_insert(0) += 1;
        }

        let total_query_terms: usize = query_term_counts.values().sum();
        let mut query_vector = HashMap::new();
        for (term, count) in &query_term_counts {
            // Simple TF weighting for query
            let tf = *count as f64 / total_query_terms as f64;
            query_vector.insert(term.clone(), tf);
        }

        // Calculate scores using linear classifier (simplified SVM)
        let mut scored_docs: Vec<(usize, f64)> = self
            .feature_vectors
            .iter()
            .enumerate()
            .map(|(idx, doc_vector)| {
                let score = Self::linear_score(&query_vector, doc_vector);
                (idx, score)
            })
            .collect();

        // Sort by score (descending)
        scored_docs.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        // Return top_k documents
        let top_k = self.config.top_k.min(scored_docs.len());
        let mut results = Vec::new();
        for (doc_id, score) in scored_docs.into_iter().take(top_k) {
            if let Some(doc) = self.documents.get(doc_id) {
                let mut doc = doc.clone();
                // Add score to metadata
                doc.metadata
                    .insert("svm_score".to_string(), Value::from(score));
                results.push(doc);
            }
        }

        Ok(results)
    }
}
