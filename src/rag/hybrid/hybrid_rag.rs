use std::sync::Arc;

use async_trait::async_trait;

use crate::{
    chain::{Chain, ConversationalRetrieverChainBuilder},
    error::RetrieverError,
    language_models::llm::LLM,
    memory::SimpleMemory,
    prompt::PromptArgs,
    schemas::{BaseMemory, Document, Retriever},
};

// Wrapper to convert Arc<dyn Retriever> to Box<dyn Retriever>
struct RetrieverWrapper(Arc<dyn Retriever>);

#[async_trait]
impl Retriever for RetrieverWrapper {
    async fn get_relevant_documents(&self, query: &str) -> Result<Vec<Document>, RetrieverError> {
        self.0.get_relevant_documents(query).await
    }
}

use super::{
    answer_validator::AnswerValidator, query_enhancer::QueryEnhancer,
    retrieval_validator::RetrievalValidator,
};
use crate::rag::RAGError;

/// Configuration for Hybrid RAG
pub struct HybridRAGConfig {
    /// Maximum number of retrieval retries
    pub max_retrieval_retries: usize,
    /// Maximum number of generation retries
    pub max_generation_retries: usize,
    /// Whether to enable query enhancement
    pub enable_query_enhancement: bool,
    /// Whether to enable retrieval validation
    pub enable_retrieval_validation: bool,
    /// Whether to enable answer validation
    pub enable_answer_validation: bool,
}

impl Default for HybridRAGConfig {
    fn default() -> Self {
        Self {
            max_retrieval_retries: 2,
            max_generation_retries: 2,
            enable_query_enhancement: true,
            enable_retrieval_validation: true,
            enable_answer_validation: true,
        }
    }
}

/// Hybrid RAG implementation that combines 2-Step and Agentic RAG with validation steps.
///
/// Flow:
/// 1. Query Enhancement (optional)
/// 2. Retrieval
/// 3. Retrieval Validation → [If fails] → Refine Query → Retrieval (retry)
/// 4. Generation
/// 5. Answer Validation → [If fails] → Regenerate or Refine
pub struct HybridRAG {
    /// The retriever
    retriever: Arc<dyn Retriever>,
    /// The LLM for generation
    llm: Box<dyn LLM>,
    /// Memory for conversation history
    memory: Arc<tokio::sync::Mutex<dyn BaseMemory>>,
    /// Query enhancer (optional)
    query_enhancer: Option<Arc<dyn QueryEnhancer>>,
    /// Retrieval validator (optional)
    retrieval_validator: Option<Arc<dyn RetrievalValidator>>,
    /// Answer validator (optional)
    answer_validator: Option<Arc<dyn AnswerValidator>>,
    /// Configuration
    config: HybridRAGConfig,
}

impl HybridRAG {
    /// Create a new HybridRAG
    pub fn new(
        retriever: Arc<dyn Retriever>,
        llm: Box<dyn LLM>,
        memory: Arc<tokio::sync::Mutex<dyn BaseMemory>>,
    ) -> Self {
        Self {
            retriever,
            llm,
            memory,
            query_enhancer: None,
            retrieval_validator: None,
            answer_validator: None,
            config: HybridRAGConfig::default(),
        }
    }

    /// Set query enhancer
    pub fn with_query_enhancer(mut self, enhancer: Arc<dyn QueryEnhancer>) -> Self {
        self.query_enhancer = Some(enhancer);
        self
    }

    /// Set retrieval validator
    pub fn with_retrieval_validator(mut self, validator: Arc<dyn RetrievalValidator>) -> Self {
        self.retrieval_validator = Some(validator);
        self
    }

    /// Set answer validator
    pub fn with_answer_validator(mut self, validator: Arc<dyn AnswerValidator>) -> Self {
        self.answer_validator = Some(validator);
        self
    }

    /// Set configuration
    pub fn with_config(mut self, config: HybridRAGConfig) -> Self {
        self.config = config;
        self
    }

    /// Execute the Hybrid RAG pipeline
    pub async fn invoke(&self, query: &str) -> Result<String, RAGError> {
        let mut current_query = query.to_string();
        let mut documents;

        // Step 1: Query Enhancement (optional)
        if self.config.enable_query_enhancement {
            if let Some(enhancer) = &self.query_enhancer {
                let enhanced = enhancer.enhance(&current_query).await?;
                current_query = enhanced.query;
            }
        }

        // Step 2 & 3: Retrieval with Validation (with retries)
        let mut retrieval_attempts = 0;
        loop {
            documents = self
                .retriever
                .get_relevant_documents(&current_query)
                .await
                .map_err(|e| RAGError::RetrieverError(e.to_string()))?;

            // Validate retrieval if enabled
            if self.config.enable_retrieval_validation {
                if let Some(validator) = &self.retrieval_validator {
                    let validation = validator.validate(&current_query, &documents).await?;

                    if !validation.is_valid
                        && retrieval_attempts < self.config.max_retrieval_retries
                    {
                        // Try to refine query based on suggestions
                        if let Some(suggestion) = validation.suggestions.first() {
                            current_query = format!("{} {}", current_query, suggestion);
                        }
                        retrieval_attempts += 1;
                        continue;
                    } else if !validation.is_valid {
                        // Max retries reached, proceed with what we have
                        log::warn!(
                            "Retrieval validation failed but max retries reached: {:?}",
                            validation.feedback
                        );
                    }
                }
            }

            break;
        }

        // Step 4: Generation
        let mut generation_attempts = 0;
        let mut answer;

        loop {
            // Build a simple chain for generation
            let retriever_box: Box<dyn Retriever> =
                Box::new(RetrieverWrapper(self.retriever.clone()));
            let chain = ConversationalRetrieverChainBuilder::new()
                .llm(self.llm.clone_box())
                .retriever(retriever_box)
                .memory(Arc::clone(&self.memory))
                .rephrase_question(false) // Already enhanced
                .build()
                .map_err(|e| RAGError::ChainError(e))?;

            // Create prompt args with the query
            let mut prompt_args = PromptArgs::new();
            prompt_args.insert("question".to_string(), serde_json::json!(current_query));

            answer = chain
                .invoke(prompt_args)
                .await
                .map_err(|e| RAGError::ChainError(e))?;

            // Step 5: Answer Validation (optional)
            if self.config.enable_answer_validation {
                if let Some(validator) = &self.answer_validator {
                    let validation = validator
                        .validate(&current_query, &answer, &documents)
                        .await?;

                    if !validation.is_valid
                        && generation_attempts < self.config.max_generation_retries
                    {
                        // Try to refine query or regenerate
                        if let Some(suggestion) = validation.suggestions.first() {
                            current_query = format!("{} {}", current_query, suggestion);
                        }
                        generation_attempts += 1;
                        continue;
                    } else if !validation.is_valid {
                        log::warn!(
                            "Answer validation failed but max retries reached: {:?}",
                            validation.feedback
                        );
                    }
                }
            }

            break;
        }

        Ok(answer)
    }
}

/// Builder for creating HybridRAG
pub struct HybridRAGBuilder {
    retriever: Option<Arc<dyn Retriever>>,
    llm: Option<Box<dyn LLM>>,
    memory: Option<Arc<tokio::sync::Mutex<dyn BaseMemory>>>,
    query_enhancer: Option<Arc<dyn QueryEnhancer>>,
    retrieval_validator: Option<Arc<dyn RetrievalValidator>>,
    answer_validator: Option<Arc<dyn AnswerValidator>>,
    config: HybridRAGConfig,
}

impl HybridRAGBuilder {
    /// Create a new HybridRAGBuilder
    pub fn new() -> Self {
        Self {
            retriever: None,
            llm: None,
            memory: None,
            query_enhancer: None,
            retrieval_validator: None,
            answer_validator: None,
            config: HybridRAGConfig::default(),
        }
    }

    /// Set the retriever
    pub fn with_retriever(mut self, retriever: Arc<dyn Retriever>) -> Self {
        self.retriever = Some(retriever);
        self
    }

    /// Set the LLM
    pub fn with_llm<L: Into<Box<dyn LLM>>>(mut self, llm: L) -> Self {
        self.llm = Some(llm.into());
        self
    }

    /// Set the memory
    pub fn with_memory(mut self, memory: Arc<tokio::sync::Mutex<dyn BaseMemory>>) -> Self {
        self.memory = Some(memory);
        self
    }

    /// Set query enhancer
    pub fn with_query_enhancer(mut self, enhancer: Arc<dyn QueryEnhancer>) -> Self {
        self.query_enhancer = Some(enhancer);
        self
    }

    /// Set retrieval validator
    pub fn with_retrieval_validator(mut self, validator: Arc<dyn RetrievalValidator>) -> Self {
        self.retrieval_validator = Some(validator);
        self
    }

    /// Set answer validator
    pub fn with_answer_validator(mut self, validator: Arc<dyn AnswerValidator>) -> Self {
        self.answer_validator = Some(validator);
        self
    }

    /// Set configuration
    pub fn with_config(mut self, config: HybridRAGConfig) -> Self {
        self.config = config;
        self
    }

    /// Build the HybridRAG instance
    pub fn build(self) -> Result<HybridRAG, RAGError> {
        let retriever = self
            .retriever
            .ok_or_else(|| RAGError::InvalidConfiguration("Retriever must be set".to_string()))?;

        let llm = self
            .llm
            .ok_or_else(|| RAGError::InvalidConfiguration("LLM must be set".to_string()))?;

        let memory = self
            .memory
            .unwrap_or_else(|| Arc::new(tokio::sync::Mutex::new(SimpleMemory::new())));

        let mut hybrid_rag = HybridRAG::new(retriever, llm, memory);

        if let Some(enhancer) = self.query_enhancer {
            hybrid_rag = hybrid_rag.with_query_enhancer(enhancer);
        }

        if let Some(validator) = self.retrieval_validator {
            hybrid_rag = hybrid_rag.with_retrieval_validator(validator);
        }

        if let Some(validator) = self.answer_validator {
            hybrid_rag = hybrid_rag.with_answer_validator(validator);
        }

        hybrid_rag = hybrid_rag.with_config(self.config);

        Ok(hybrid_rag)
    }
}

impl Default for HybridRAGBuilder {
    fn default() -> Self {
        Self::new()
    }
}
