use std::collections::HashMap;
use std::error::Error;
use std::sync::Arc;

use async_trait::async_trait;

use crate::error::RetrieverError;
use crate::language_models::llm::LLM;
use crate::schemas::{Document, Retriever};

/// Configuration for Multi Query retriever
#[derive(Debug, Clone)]
pub struct MultiQueryRetrieverConfig {
    /// Number of query variations to generate
    pub num_queries: usize,
    /// Prompt template for generating query variations
    pub prompt_template: Option<String>,
}

impl Default for MultiQueryRetrieverConfig {
    fn default() -> Self {
        Self {
            num_queries: 3,
            prompt_template: None,
        }
    }
}

/// Multi Query retriever that generates multiple query variations and merges results
pub struct MultiQueryRetriever {
    base_retriever: Arc<dyn Retriever>,
    llm: Arc<dyn LLM>,
    config: MultiQueryRetrieverConfig,
}

impl MultiQueryRetriever {
    /// Create a new multi query retriever
    pub fn new(base_retriever: Arc<dyn Retriever>, llm: Arc<dyn LLM>) -> Self {
        Self::with_config(base_retriever, llm, MultiQueryRetrieverConfig::default())
    }

    /// Create a new multi query retriever with custom config
    pub fn with_config(
        base_retriever: Arc<dyn Retriever>,
        llm: Arc<dyn LLM>,
        config: MultiQueryRetrieverConfig,
    ) -> Self {
        Self {
            base_retriever,
            llm,
            config,
        }
    }

    /// Generate multiple query variations using LLM
    async fn generate_queries(&self, original_query: &str) -> Result<Vec<String>, Box<dyn Error>> {
        let prompt = self.config.prompt_template.as_ref().map(|t| {
            t.replace("{query}", original_query)
                .replace("{num_queries}", &self.config.num_queries.to_string())
        }).unwrap_or_else(|| {
            format!(
                "You are an AI language model assistant. Your task is to generate {} different versions \
                of the given user question to retrieve relevant documents from a vector database. \
                By generating multiple perspectives on the user question, your goal is to help \
                the user overcome some of the limitations of distance-based similarity search. \
                Provide these alternative questions separated by newlines.\n\n\
                Original question: {}\n\n\
                Alternative questions:",
                self.config.num_queries,
                original_query
            )
        });

        let messages = vec![crate::schemas::messages::Message::new_human_message(
            &prompt,
        )];
        let result = self.llm.generate(&messages).await?;

        // Parse the generated queries (split by newlines)
        let queries: Vec<String> = result
            .generation
            .lines()
            .map(|line| line.trim().to_string())
            .filter(|line| !line.is_empty())
            .take(self.config.num_queries)
            .collect();

        // Always include the original query
        let mut all_queries = vec![original_query.to_string()];
        all_queries.extend(queries);

        Ok(all_queries)
    }

    /// Merge results from multiple queries, removing duplicates
    fn merge_results(&self, all_results: Vec<Vec<Document>>) -> Vec<Document> {
        let mut seen = HashMap::new();
        let mut merged = Vec::new();

        for results in all_results {
            for doc in results {
                // Use page_content as the key for deduplication
                let key = doc.page_content.clone();
                if !seen.contains_key(&key) {
                    seen.insert(key.clone(), true);
                    merged.push(doc);
                }
            }
        }

        merged
    }
}

#[async_trait]
impl Retriever for MultiQueryRetriever {
    async fn get_relevant_documents(&self, query: &str) -> Result<Vec<Document>, RetrieverError> {
        // Generate multiple query variations
        let queries = self
            .generate_queries(query)
            .await
            .map_err(|e| RetrieverError::DocumentProcessingError(e.to_string()))?;

        // Retrieve documents for each query
        let mut all_results = Vec::new();
        for q in queries {
            match self.base_retriever.get_relevant_documents(&q).await {
                Ok(results) => all_results.push(results),
                Err(_e) => {
                    log::warn!(
                        "Failed to retrieve documents for generated query (length: {})",
                        q.len()
                    );
                    all_results.push(Vec::new());
                }
            }
        }

        // Merge and deduplicate results
        let merged = self.merge_results(all_results);

        Ok(merged)
    }
}
