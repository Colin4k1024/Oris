use std::sync::Arc;
use tokio::sync::Mutex;

use crate::{
    chain::ConversationalRetrieverChainBuilder,
    language_models::llm::LLM,
    memory::SimpleMemory,
    rag::RAGError,
    schemas::{BaseMemory, Retriever},
};

use super::two_step_rag::TwoStepRAG;

/// Builder for creating a 2-Step RAG instance.
///
/// This provides a cleaner API that aligns with LangChain Python's standard.
pub struct TwoStepRAGBuilder {
    llm: Option<Box<dyn LLM>>,
    retriever: Option<Box<dyn Retriever>>,
    memory: Option<Arc<Mutex<dyn BaseMemory>>>,
    rephrase_question: bool,
    return_source_documents: bool,
}

impl TwoStepRAGBuilder {
    /// Create a new TwoStepRAGBuilder
    pub fn new() -> Self {
        Self {
            llm: None,
            retriever: None,
            memory: None,
            rephrase_question: true,
            return_source_documents: true,
        }
    }

    /// Set the LLM for generation
    pub fn with_llm<L: Into<Box<dyn LLM>>>(mut self, llm: L) -> Self {
        self.llm = Some(llm.into());
        self
    }

    /// Set the retriever
    pub fn with_retriever<R: Into<Box<dyn Retriever>>>(mut self, retriever: R) -> Self {
        self.retriever = Some(retriever.into());
        self
    }

    /// Set the memory for conversation history
    pub fn with_memory(mut self, memory: Arc<Mutex<dyn BaseMemory>>) -> Self {
        self.memory = Some(memory);
        self
    }

    /// Set whether to rephrase the question based on chat history
    pub fn with_rephrase_question(mut self, rephrase: bool) -> Self {
        self.rephrase_question = rephrase;
        self
    }

    /// Set whether to return source documents in the result
    pub fn with_return_source_documents(mut self, return_docs: bool) -> Self {
        self.return_source_documents = return_docs;
        self
    }

    /// Build the TwoStepRAG instance
    pub fn build(self) -> Result<TwoStepRAG, RAGError> {
        let llm = self
            .llm
            .ok_or_else(|| RAGError::InvalidConfiguration("LLM must be set".to_string()))?;

        let retriever = self
            .retriever
            .ok_or_else(|| RAGError::InvalidConfiguration("Retriever must be set".to_string()))?;

        let memory = self
            .memory
            .unwrap_or_else(|| Arc::new(Mutex::new(SimpleMemory::new())));

        let chain = ConversationalRetrieverChainBuilder::new()
            .llm(llm)
            .retriever(retriever)
            .memory(memory)
            .rephrase_question(self.rephrase_question)
            .return_source_documents(self.return_source_documents)
            .build()
            .map_err(|e| RAGError::ChainError(e))?;

        Ok(TwoStepRAG::from_chain(chain))
    }
}

impl Default for TwoStepRAGBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builder_creation() {
        let builder = TwoStepRAGBuilder::new();
        assert!(builder.llm.is_none());
        assert!(builder.retriever.is_none());
    }

    #[test]
    fn test_builder_with_options() {
        let builder = TwoStepRAGBuilder::new()
            .with_rephrase_question(false)
            .with_return_source_documents(false);
        // Builder created successfully
        assert!(!builder.rephrase_question);
        assert!(!builder.return_source_documents);
    }
}
