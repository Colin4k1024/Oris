use async_trait::async_trait;

use crate::{rag::RAGError, schemas::Document};

/// Result of retrieval validation
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RetrievalValidationResult {
    /// Whether the retrieval is valid (relevant and sufficient)
    pub is_valid: bool,
    /// Confidence score (0.0 to 1.0)
    pub confidence: f64,
    /// Optional feedback message
    pub feedback: Option<String>,
    /// Suggested improvements (if validation fails)
    pub suggestions: Vec<String>,
}

impl RetrievalValidationResult {
    /// Create a valid result
    pub fn valid(confidence: f64) -> Self {
        Self {
            is_valid: true,
            confidence,
            feedback: None,
            suggestions: Vec::new(),
        }
    }

    /// Create an invalid result with suggestions
    pub fn invalid(confidence: f64, feedback: String, suggestions: Vec<String>) -> Self {
        Self {
            is_valid: false,
            confidence,
            feedback: Some(feedback),
            suggestions,
        }
    }
}

/// Trait for validating retrieved documents.
///
/// Validation checks whether retrieved documents are:
/// - Relevant to the query
/// - Sufficient to answer the query
#[async_trait]
pub trait RetrievalValidator: Send + Sync {
    /// Validate retrieved documents
    async fn validate(
        &self,
        query: &str,
        documents: &[Document],
    ) -> Result<RetrievalValidationResult, RAGError>;
}

/// Relevance validator based on similarity scores or simple heuristics
pub struct RelevanceValidator {
    /// Minimum number of documents required
    min_documents: usize,
    /// Minimum relevance threshold (if available in document metadata)
    min_relevance: Option<f64>,
}

impl RelevanceValidator {
    /// Create a new RelevanceValidator
    pub fn new() -> Self {
        Self {
            min_documents: 1,
            min_relevance: None,
        }
    }

    /// Set minimum number of documents
    pub fn with_min_documents(mut self, min: usize) -> Self {
        self.min_documents = min;
        self
    }

    /// Set minimum relevance threshold
    pub fn with_min_relevance(mut self, threshold: f64) -> Self {
        self.min_relevance = Some(threshold);
        self
    }
}

impl Default for RelevanceValidator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schemas::Document;

    #[tokio::test]
    async fn test_relevance_validator_min_documents() {
        let validator = RelevanceValidator::new().with_min_documents(2);

        let docs = vec![Document::new("test")];
        let result = validator.validate("test query", &docs).await.unwrap();
        assert!(!result.is_valid);
    }

    #[tokio::test]
    async fn test_relevance_validator_sufficient_documents() {
        let validator = RelevanceValidator::new().with_min_documents(1);

        let docs = vec![Document::new("test content")];
        let result = validator.validate("test query", &docs).await.unwrap();
        assert!(result.is_valid);
    }
}

#[async_trait]
impl RetrievalValidator for RelevanceValidator {
    async fn validate(
        &self,
        _query: &str,
        documents: &[Document],
    ) -> Result<RetrievalValidationResult, RAGError> {
        // Check minimum document count
        if documents.len() < self.min_documents {
            return Ok(RetrievalValidationResult::invalid(
                0.0,
                format!(
                    "Insufficient documents retrieved: got {}, need at least {}",
                    documents.len(),
                    self.min_documents
                ),
                vec!["Try expanding the query or using different search terms".to_string()],
            ));
        }

        // Check relevance scores if available
        if let Some(min_relevance) = self.min_relevance {
            let mut all_relevant = true;
            let mut avg_score = 0.0;
            let mut count = 0;

            for doc in documents {
                if let Some(score_val) = doc.metadata.get("score") {
                    if let Some(score) = score_val.as_f64() {
                        avg_score += score;
                        count += 1;
                        if score < min_relevance {
                            all_relevant = false;
                        }
                    }
                }
            }

            if count > 0 {
                avg_score /= count as f64;
                if !all_relevant || avg_score < min_relevance {
                    return Ok(RetrievalValidationResult::invalid(
                        avg_score,
                        format!("Some documents have low relevance scores (avg: {:.2}, min required: {:.2})", 
                            avg_score, min_relevance),
                        vec!["Try refining the query to be more specific".to_string()],
                    ));
                }
            }
        }

        // Basic validation passed
        Ok(RetrievalValidationResult::valid(0.8))
    }
}

/// LLM-based retrieval validator that uses an LLM to assess relevance
pub struct LLMRetrievalValidator {
    llm: Box<dyn crate::language_models::llm::LLM>,
    validation_prompt: Option<String>,
}

impl LLMRetrievalValidator {
    /// Create a new LLMRetrievalValidator
    pub fn new(llm: Box<dyn crate::language_models::llm::LLM>) -> Self {
        Self {
            llm,
            validation_prompt: None,
        }
    }

    /// Set a custom validation prompt
    pub fn with_prompt<S: Into<String>>(mut self, prompt: S) -> Self {
        self.validation_prompt = Some(prompt.into());
        self
    }
}

#[async_trait]
impl RetrievalValidator for LLMRetrievalValidator {
    async fn validate(
        &self,
        query: &str,
        documents: &[Document],
    ) -> Result<RetrievalValidationResult, RAGError> {
        if documents.is_empty() {
            return Ok(RetrievalValidationResult::invalid(
                0.0,
                "No documents retrieved".to_string(),
                vec!["Try expanding the query".to_string()],
            ));
        }

        // Format documents for LLM
        let doc_texts: Vec<String> = documents
            .iter()
            .take(5) // Limit to first 5 for prompt size
            .map(|doc| format!("[Document]\n{}\n", doc.page_content))
            .collect();

        let prompt = self.validation_prompt.as_deref().unwrap_or(
            "Evaluate whether the following documents are relevant and sufficient to answer the query.\n\n\
             Query: {query}\n\n\
             Documents:\n{documents}\n\n\
             Respond with JSON: {{\"is_valid\": true/false, \"confidence\": 0.0-1.0, \"feedback\": \"...\", \"suggestions\": [\"...\"]}}"
        );

        let formatted_prompt = prompt
            .replace("{query}", query)
            .replace("{documents}", &doc_texts.join("\n---\n"));

        let response = self
            .llm
            .invoke(&formatted_prompt)
            .await
            .map_err(|e| RAGError::RetrievalValidationError(format!("LLM error: {}", e)))?;

        // Try to parse JSON response
        match serde_json::from_str::<RetrievalValidationResult>(&response) {
            Ok(result) => Ok(result),
            Err(_) => {
                // Fallback: simple heuristic based on response
                let is_valid = response.to_lowercase().contains("valid")
                    || response.to_lowercase().contains("yes")
                    || response.to_lowercase().contains("sufficient");
                Ok(RetrievalValidationResult {
                    is_valid,
                    confidence: if is_valid { 0.7 } else { 0.3 },
                    feedback: Some(response),
                    suggestions: Vec::new(),
                })
            }
        }
    }
}
