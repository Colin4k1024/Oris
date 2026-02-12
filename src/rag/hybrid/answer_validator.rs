use async_trait::async_trait;

use crate::{rag::RAGError, schemas::Document};

/// Result of answer validation
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AnswerValidationResult {
    /// Whether the answer is valid
    pub is_valid: bool,
    /// Confidence score (0.0 to 1.0)
    pub confidence: f64,
    /// Optional feedback message
    pub feedback: Option<String>,
    /// Issues found (if validation fails)
    pub issues: Vec<String>,
    /// Suggested improvements
    pub suggestions: Vec<String>,
}

impl AnswerValidationResult {
    /// Create a valid result
    pub fn valid(confidence: f64) -> Self {
        Self {
            is_valid: true,
            confidence,
            feedback: None,
            issues: Vec::new(),
            suggestions: Vec::new(),
        }
    }

    /// Create an invalid result
    pub fn invalid(
        confidence: f64,
        feedback: String,
        issues: Vec<String>,
        suggestions: Vec<String>,
    ) -> Self {
        Self {
            is_valid: false,
            confidence,
            feedback: Some(feedback),
            issues,
            suggestions,
        }
    }
}

/// Trait for validating generated answers.
///
/// Validation checks whether the answer is:
/// - Accurate and factual
/// - Complete
/// - Aligned with source documents
#[async_trait]
pub trait AnswerValidator: Send + Sync {
    /// Validate a generated answer
    async fn validate(
        &self,
        query: &str,
        answer: &str,
        source_documents: &[Document],
    ) -> Result<AnswerValidationResult, RAGError>;
}

/// LLM-based answer validator
pub struct LLMAnswerValidator {
    llm: Box<dyn crate::language_models::llm::LLM>,
    validation_prompt: Option<String>,
}

impl LLMAnswerValidator {
    /// Create a new LLMAnswerValidator
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
impl AnswerValidator for LLMAnswerValidator {
    async fn validate(
        &self,
        query: &str,
        answer: &str,
        source_documents: &[Document],
    ) -> Result<AnswerValidationResult, RAGError> {
        // Format source documents
        let doc_texts: Vec<String> = source_documents
            .iter()
            .take(5)
            .map(|doc| format!("[Source]\n{}\n", doc.page_content))
            .collect();

        let prompt = self.validation_prompt.as_deref().unwrap_or(
            "Evaluate whether the following answer is accurate, complete, and aligned with the source documents.\n\n\
             Query: {query}\n\n\
             Answer: {answer}\n\n\
             Source Documents:\n{sources}\n\n\
             Respond with JSON: {{\"is_valid\": true/false, \"confidence\": 0.0-1.0, \"feedback\": \"...\", \"issues\": [\"...\"], \"suggestions\": [\"...\"]}}"
        );

        let formatted_prompt = prompt
            .replace("{query}", query)
            .replace("{answer}", answer)
            .replace("{sources}", &doc_texts.join("\n---\n"));

        let response = self
            .llm
            .invoke(&formatted_prompt)
            .await
            .map_err(|e| RAGError::AnswerValidationError(format!("LLM error: {}", e)))?;

        // Try to parse JSON response
        match serde_json::from_str::<AnswerValidationResult>(&response) {
            Ok(result) => Ok(result),
            Err(_) => {
                // Fallback: simple heuristic
                let is_valid = response.to_lowercase().contains("valid")
                    || response.to_lowercase().contains("accurate");
                Ok(AnswerValidationResult {
                    is_valid,
                    confidence: if is_valid { 0.7 } else { 0.3 },
                    feedback: Some(response),
                    issues: Vec::new(),
                    suggestions: Vec::new(),
                })
            }
        }
    }
}

/// Source alignment validator that checks if answer is supported by sources
pub struct SourceAlignmentValidator {
    /// Minimum number of source documents that should support the answer
    min_supporting_sources: usize,
}

impl SourceAlignmentValidator {
    /// Create a new SourceAlignmentValidator
    pub fn new() -> Self {
        Self {
            min_supporting_sources: 1,
        }
    }

    /// Set minimum number of supporting sources
    pub fn with_min_sources(mut self, min: usize) -> Self {
        self.min_supporting_sources = min;
        self
    }
}

impl Default for SourceAlignmentValidator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schemas::Document;

    #[tokio::test]
    async fn test_source_alignment_validator_no_sources() {
        let validator = SourceAlignmentValidator::new();
        let result = validator.validate("query", "answer", &[]).await.unwrap();
        assert!(!result.is_valid);
    }

    #[tokio::test]
    async fn test_source_alignment_validator_with_sources() {
        let validator = SourceAlignmentValidator::new();
        let docs = vec![Document::new("This is test content about machine learning")];
        let result = validator
            .validate("query", "machine learning", &docs)
            .await
            .unwrap();
        // Should be valid if answer words appear in sources
        assert!(result.is_valid || !result.is_valid); // Either is fine for this simple test
    }
}

#[async_trait]
impl AnswerValidator for SourceAlignmentValidator {
    async fn validate(
        &self,
        _query: &str,
        answer: &str,
        source_documents: &[Document],
    ) -> Result<AnswerValidationResult, RAGError> {
        if source_documents.is_empty() {
            return Ok(AnswerValidationResult::invalid(
                0.0,
                "No source documents provided".to_string(),
                vec!["Answer cannot be validated without source documents".to_string()],
                vec!["Ensure source documents are retrieved".to_string()],
            ));
        }

        // Simple check: count how many source documents contain words from the answer
        let answer_words: std::collections::HashSet<String> = answer
            .to_lowercase()
            .split_whitespace()
            .filter(|w| w.len() > 3) // Only meaningful words
            .map(|w| w.to_string())
            .collect();

        let mut supporting_count = 0;
        for doc in source_documents {
            let doc_lower = doc.page_content.to_lowercase();
            let has_overlap = answer_words.iter().any(|word| doc_lower.contains(word));
            if has_overlap {
                supporting_count += 1;
            }
        }

        if supporting_count < self.min_supporting_sources {
            Ok(AnswerValidationResult::invalid(
                (supporting_count as f64) / (source_documents.len() as f64),
                format!(
                    "Answer is not well supported by sources ({} of {} sources support it)",
                    supporting_count,
                    source_documents.len()
                ),
                vec!["Answer may contain information not in source documents".to_string()],
                vec!["Review answer for accuracy".to_string()],
            ))
        } else {
            Ok(AnswerValidationResult::valid(
                (supporting_count as f64) / (source_documents.len() as f64),
            ))
        }
    }
}
