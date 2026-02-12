use std::error::Error;
use std::sync::Arc;

use async_trait::async_trait;

use crate::embedding::embedder_trait::Embedder;
use crate::error::RetrieverError;
use crate::schemas::{Document, Retriever};

/// Configuration for embeddings redundant filter
#[derive(Debug, Clone)]
pub struct EmbeddingsRedundantFilterConfig {
    /// Similarity threshold for considering documents redundant (0.0 to 1.0)
    pub similarity_threshold: f64,
    /// Maximum number of documents to return
    pub max_docs: Option<usize>,
}

impl Default for EmbeddingsRedundantFilterConfig {
    fn default() -> Self {
        Self {
            similarity_threshold: 0.95,
            max_docs: None,
        }
    }
}

/// Embeddings redundant filter that removes similar documents based on embedding similarity
pub struct EmbeddingsRedundantFilter {
    base_retriever: Arc<dyn Retriever>,
    embedder: Arc<dyn Embedder>,
    config: EmbeddingsRedundantFilterConfig,
}

impl EmbeddingsRedundantFilter {
    /// Create a new embeddings redundant filter
    pub fn new(base_retriever: Arc<dyn Retriever>, embedder: Arc<dyn Embedder>) -> Self {
        Self::with_config(
            base_retriever,
            embedder,
            EmbeddingsRedundantFilterConfig::default(),
        )
    }

    /// Create a new embeddings redundant filter with custom config
    pub fn with_config(
        base_retriever: Arc<dyn Retriever>,
        embedder: Arc<dyn Embedder>,
        config: EmbeddingsRedundantFilterConfig,
    ) -> Self {
        Self {
            base_retriever,
            embedder,
            config,
        }
    }

    /// Calculate cosine similarity between two vectors
    fn cosine_similarity(a: &Vec<f64>, b: &Vec<f64>) -> f64 {
        if a.len() != b.len() {
            return 0.0;
        }

        let dot_product: f64 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
        let norm_a: f64 = a.iter().map(|x| x * x).sum::<f64>().sqrt();
        let norm_b: f64 = b.iter().map(|x| x * x).sum::<f64>().sqrt();

        if norm_a == 0.0 || norm_b == 0.0 {
            return 0.0;
        }

        dot_product / (norm_a * norm_b)
    }

    /// Filter redundant documents based on embedding similarity
    async fn filter_redundant(
        &self,
        documents: Vec<Document>,
    ) -> Result<Vec<Document>, Box<dyn Error>> {
        if documents.is_empty() {
            return Ok(documents);
        }

        // Generate embeddings for all documents
        let texts: Vec<String> = documents.iter().map(|d| d.page_content.clone()).collect();
        let embeddings = self.embedder.embed_documents(&texts).await?;

        // Filter redundant documents
        let mut filtered = Vec::new();
        let mut used_indices = Vec::new();

        for (i, doc) in documents.iter().enumerate() {
            if let Some(embedding) = embeddings.get(i) {
                let mut is_redundant = false;

                // Check similarity with already included documents
                for &used_idx in &used_indices {
                    if let Some(existing_embedding) = embeddings.get(used_idx) {
                        let similarity = Self::cosine_similarity(embedding, existing_embedding);
                        if similarity >= self.config.similarity_threshold {
                            is_redundant = true;
                            break;
                        }
                    }
                }

                if !is_redundant {
                    used_indices.push(i);
                    filtered.push(doc.clone());
                }
            } else {
                // If embedding generation failed, include the document anyway
                used_indices.push(i);
                filtered.push(doc.clone());
            }
        }

        // Apply max_docs limit if configured
        if let Some(max) = self.config.max_docs {
            filtered.truncate(max);
        }

        Ok(filtered)
    }
}

#[async_trait]
impl Retriever for EmbeddingsRedundantFilter {
    async fn get_relevant_documents(&self, query: &str) -> Result<Vec<Document>, RetrieverError> {
        // Get documents from base retriever
        let documents = self.base_retriever.get_relevant_documents(query).await?;

        // Filter redundant documents
        self.filter_redundant(documents)
            .await
            .map_err(|e| RetrieverError::DocumentProcessingError(e.to_string()))
    }
}
