use crate::{
    chain::{Chain, ChainError, ConversationalRetrieverChain},
    language_models::GenerateResult,
    prompt::PromptArgs,
};

use crate::rag::RAGError;

/// Optimized 2-Step RAG implementation.
///
/// This wraps the existing `ConversationalRetrieverChain` with a cleaner API
/// that aligns with LangChain Python's standard.
///
/// In 2-Step RAG, retrieval always happens before generation, making it
/// simple and predictable.
pub struct TwoStepRAG {
    chain: ConversationalRetrieverChain,
}

impl TwoStepRAG {
    /// Create a new TwoStepRAG from an existing chain
    pub fn from_chain(chain: ConversationalRetrieverChain) -> Self {
        Self { chain }
    }

    /// Invoke the RAG chain with a question
    pub async fn invoke(&self, question: &str) -> Result<String, RAGError> {
        let mut prompt_args = PromptArgs::new();
        prompt_args.insert("question".to_string(), serde_json::json!(question));

        let result = self.chain.invoke(prompt_args).await?;
        Ok(result)
    }

    /// Call the RAG chain and get full result
    pub async fn call(&self, question: &str) -> Result<GenerateResult, RAGError> {
        let mut prompt_args = PromptArgs::new();
        prompt_args.insert("question".to_string(), serde_json::json!(question));

        let result = self.chain.call(prompt_args).await?;
        Ok(result)
    }

    /// Get a reference to the underlying chain
    pub fn chain(&self) -> &ConversationalRetrieverChain {
        &self.chain
    }
}

#[async_trait::async_trait]
impl Chain for TwoStepRAG {
    async fn call(&self, input_variables: PromptArgs) -> Result<GenerateResult, ChainError> {
        self.chain.call(input_variables).await
    }

    async fn invoke(&self, input_variables: PromptArgs) -> Result<String, ChainError> {
        self.chain.invoke(input_variables).await
    }
}
