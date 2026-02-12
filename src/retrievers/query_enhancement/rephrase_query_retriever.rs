use std::error::Error;
use std::sync::Arc;

use async_trait::async_trait;

use crate::error::RetrieverError;
use crate::language_models::llm::LLM;
use crate::schemas::{Document, Retriever};

/// Configuration for RePhrase Query retriever
#[derive(Debug, Clone)]
pub struct RePhraseQueryRetrieverConfig {
    /// Prompt template for query rephrasing
    pub prompt_template: Option<String>,
}

impl Default for RePhraseQueryRetrieverConfig {
    fn default() -> Self {
        Self {
            prompt_template: None, // Will use default template
        }
    }
}

/// RePhrase Query retriever that uses LLM to rephrase queries before retrieval
pub struct RePhraseQueryRetriever {
    base_retriever: Arc<dyn Retriever>,
    llm: Arc<dyn LLM>,
    config: RePhraseQueryRetrieverConfig,
}

impl RePhraseQueryRetriever {
    /// Create a new rephrase query retriever
    pub fn new(base_retriever: Arc<dyn Retriever>, llm: Arc<dyn LLM>) -> Self {
        Self::with_config(base_retriever, llm, RePhraseQueryRetrieverConfig::default())
    }

    /// Create a new rephrase query retriever with custom config
    pub fn with_config(
        base_retriever: Arc<dyn Retriever>,
        llm: Arc<dyn LLM>,
        config: RePhraseQueryRetrieverConfig,
    ) -> Self {
        Self {
            base_retriever,
            llm,
            config,
        }
    }

    /// Rephrase the query using LLM
    async fn rephrase_query(&self, query: &str) -> Result<String, Box<dyn Error>> {
        let prompt = self.config.prompt_template.as_ref().map(|t| {
            t.replace("{query}", query)
        }).unwrap_or_else(|| {
            format!(
                "Rephrase the following search query to make it more effective for information retrieval. \
                Keep the core meaning but improve clarity and specificity.\n\n\
                Original query: {}\n\n\
                Rephrased query:",
                query
            )
        });

        let messages = vec![crate::schemas::messages::Message::new_human_message(
            &prompt,
        )];
        let result = self.llm.generate(&messages).await?;

        Ok(result.generation.trim().to_string())
    }
}

#[async_trait]
impl Retriever for RePhraseQueryRetriever {
    async fn get_relevant_documents(&self, query: &str) -> Result<Vec<Document>, RetrieverError> {
        // Rephrase the query
        let rephrased_query = self
            .rephrase_query(query)
            .await
            .map_err(|e| RetrieverError::DocumentProcessingError(e.to_string()))?;

        // Use rephrased query for retrieval
        self.base_retriever
            .get_relevant_documents(&rephrased_query)
            .await
    }
}
