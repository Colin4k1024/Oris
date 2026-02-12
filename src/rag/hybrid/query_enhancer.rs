use async_trait::async_trait;

use crate::rag::RAGError;

/// Result of query enhancement
#[derive(Debug, Clone)]
pub struct EnhancedQuery {
    /// The enhanced query
    pub query: String,
    /// Optional metadata about the enhancement
    pub metadata: Option<std::collections::HashMap<String, String>>,
}

impl EnhancedQuery {
    /// Create a new EnhancedQuery
    pub fn new(query: String) -> Self {
        Self {
            query,
            metadata: None,
        }
    }

    /// Create with metadata
    pub fn with_metadata(
        query: String,
        metadata: std::collections::HashMap<String, String>,
    ) -> Self {
        Self {
            query,
            metadata: Some(metadata),
        }
    }
}

/// Trait for enhancing queries to improve retrieval quality.
///
/// Query enhancement can involve:
/// - Rewriting unclear queries
/// - Generating multiple query variations
/// - Expanding queries with additional context
#[async_trait]
pub trait QueryEnhancer: Send + Sync {
    /// Enhance a query
    async fn enhance(&self, query: &str) -> Result<EnhancedQuery, RAGError>;

    /// Generate multiple query variations (optional)
    async fn enhance_multi(&self, query: &str) -> Result<Vec<EnhancedQuery>, RAGError> {
        // Default implementation returns single enhanced query
        let enhanced = self.enhance(query).await?;
        Ok(vec![enhanced])
    }
}

/// LLM-based query enhancer that uses an LLM to rewrite and improve queries
pub struct LLMQueryEnhancer {
    llm: Box<dyn crate::language_models::llm::LLM>,
    enhancement_prompt: Option<String>,
}

impl LLMQueryEnhancer {
    /// Create a new LLMQueryEnhancer
    pub fn new(llm: Box<dyn crate::language_models::llm::LLM>) -> Self {
        Self {
            llm,
            enhancement_prompt: None,
        }
    }

    /// Set a custom enhancement prompt
    pub fn with_prompt<S: Into<String>>(mut self, prompt: S) -> Self {
        self.enhancement_prompt = Some(prompt.into());
        self
    }
}

#[async_trait]
impl QueryEnhancer for LLMQueryEnhancer {
    async fn enhance(&self, query: &str) -> Result<EnhancedQuery, RAGError> {
        let prompt = self.enhancement_prompt.as_deref().unwrap_or(
            "Rewrite the following query to be more specific and effective for document retrieval. \
             Keep the core intent but make it clearer and more searchable. \
             Return only the enhanced query, no explanation.\n\nQuery: {query}\n\nEnhanced query:"
        );

        let enhanced_prompt = prompt.replace("{query}", query);
        let enhanced_query = self
            .llm
            .invoke(&enhanced_prompt)
            .await
            .map_err(|e| RAGError::QueryEnhancementError(format!("LLM error: {}", e)))?;

        Ok(EnhancedQuery::new(enhanced_query.trim().to_string()))
    }

    async fn enhance_multi(&self, query: &str) -> Result<Vec<EnhancedQuery>, RAGError> {
        let prompt = format!(
            "Generate 3 different variations of the following query for document retrieval. \
             Each variation should approach the query from a different angle. \
             Return only the queries, one per line.\n\nOriginal query: {}\n\nVariations:",
            query
        );

        let response = self
            .llm
            .invoke(&prompt)
            .await
            .map_err(|e| RAGError::QueryEnhancementError(format!("LLM error: {}", e)))?;

        let variations: Vec<EnhancedQuery> = response
            .lines()
            .map(|line| line.trim())
            .filter(|line| !line.is_empty())
            .take(3)
            .map(|q| EnhancedQuery::new(q.to_string()))
            .collect();

        if variations.is_empty() {
            // Fallback to single enhancement
            self.enhance(query).await.map(|e| vec![e])
        } else {
            Ok(variations)
        }
    }
}

/// Keyword-based query enhancer that expands queries with related keywords
pub struct KeywordQueryEnhancer {
    /// Map of terms to related keywords
    keyword_expansions: std::collections::HashMap<String, Vec<String>>,
}

impl KeywordQueryEnhancer {
    /// Create a new KeywordQueryEnhancer
    pub fn new() -> Self {
        Self {
            keyword_expansions: std::collections::HashMap::new(),
        }
    }

    /// Add keyword expansions
    pub fn with_expansions(
        mut self,
        expansions: std::collections::HashMap<String, Vec<String>>,
    ) -> Self {
        self.keyword_expansions = expansions;
        self
    }

    /// Add a single expansion
    pub fn add_expansion(mut self, term: String, related: Vec<String>) -> Self {
        self.keyword_expansions.insert(term, related);
        self
    }
}

impl Default for KeywordQueryEnhancer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_keyword_query_enhancer() {
        let mut expansions = std::collections::HashMap::new();
        expansions.insert(
            "rust".to_string(),
            vec!["systems".to_string(), "performance".to_string()],
        );

        let enhancer = KeywordQueryEnhancer::new().with_expansions(expansions);

        let result = enhancer.enhance("rust programming").await.unwrap();
        assert!(result.query.contains("rust"));
        // Should contain expanded terms
        assert!(result.query.contains("systems") || result.query.contains("performance"));
    }

    #[test]
    fn test_enhanced_query() {
        let query = EnhancedQuery::new("test query".to_string());
        assert_eq!(query.query, "test query");
        assert!(query.metadata.is_none());
    }
}

#[async_trait]
impl QueryEnhancer for KeywordQueryEnhancer {
    async fn enhance(&self, query: &str) -> Result<EnhancedQuery, RAGError> {
        let query_lower = query.to_lowercase();
        let mut enhanced_terms: Vec<String> =
            query.split_whitespace().map(|s| s.to_string()).collect();

        // Expand with related keywords
        for (term, related) in &self.keyword_expansions {
            if query_lower.contains(&term.to_lowercase()) {
                enhanced_terms.extend(related.iter().cloned());
            }
        }

        let enhanced_query = enhanced_terms.join(" ");
        Ok(EnhancedQuery::new(enhanced_query))
    }
}
