use std::collections::HashMap;
use std::sync::Arc;

use crate::{
    agent::UnifiedAgent,
    chain::ChainError,
    schemas::{messages::Message, Retriever},
    tools::Tool,
};

use super::retriever_tool::RetrieverTool;
use crate::rag::RAGError;

/// Information about a retriever to be added to an agent
pub struct RetrieverInfo {
    /// The retriever instance
    pub retriever: Arc<dyn Retriever>,
    /// Name of the retriever (used as tool name)
    pub name: String,
    /// Description of what this retriever does
    pub description: String,
    /// Maximum number of documents to retrieve
    pub max_docs: usize,
}

impl RetrieverInfo {
    /// Create a new RetrieverInfo
    pub fn new(retriever: Arc<dyn Retriever>, name: String, description: String) -> Self {
        Self {
            retriever,
            name,
            description,
            max_docs: 5,
        }
    }

    /// Set the maximum number of documents
    pub fn with_max_docs(mut self, max_docs: usize) -> Self {
        self.max_docs = max_docs;
        self
    }
}

/// An agent wrapper that supports Agentic RAG.
///
/// This implements the Agentic RAG pattern where an agent decides when
/// and how to retrieve information during reasoning, rather than always
/// retrieving before generation.
pub struct AgenticRAG {
    /// The base agent
    agent: Arc<UnifiedAgent>,
    /// Map of retriever names to retriever tools
    retriever_tools: HashMap<String, Arc<dyn Tool>>,
}

impl AgenticRAG {
    /// Create a new AgenticRAG from an agent with retriever tools
    pub fn new(agent: Arc<UnifiedAgent>, retriever_tools: Vec<Arc<dyn Tool>>) -> Self {
        let mut tool_map = HashMap::new();
        for tool in retriever_tools {
            tool_map.insert(tool.name(), tool);
        }
        Self {
            agent,
            retriever_tools: tool_map,
        }
    }

    /// Invoke the agent with messages
    pub async fn invoke_messages(&self, messages: Vec<Message>) -> Result<String, RAGError> {
        self.agent
            .invoke_messages(messages)
            .await
            .map_err(|e| RAGError::ChainError(e))
    }

    /// Get a reference to the underlying agent
    pub fn agent(&self) -> &Arc<UnifiedAgent> {
        &self.agent
    }

    /// Get a retriever tool by name
    pub fn get_retriever_tool(&self, name: &str) -> Option<&Arc<dyn Tool>> {
        self.retriever_tools.get(name)
    }
}

/// Builder for creating AgenticRAG
pub struct AgenticRAGBuilder {
    /// Base agent (optional, can be created from model string)
    agent: Option<Arc<UnifiedAgent>>,
    /// Model string (if agent is not provided)
    model: Option<String>,
    /// System prompt
    system_prompt: Option<String>,
    /// Retrievers to add as tools
    retrievers: Vec<RetrieverInfo>,
    /// Additional tools (non-retriever tools)
    additional_tools: Vec<Arc<dyn Tool>>,
}

impl AgenticRAGBuilder {
    /// Create a new AgenticRAGBuilder
    pub fn new() -> Self {
        Self {
            agent: None,
            model: None,
            system_prompt: None,
            retrievers: Vec::new(),
            additional_tools: Vec::new(),
        }
    }

    /// Set the base agent
    pub fn with_agent(mut self, agent: Arc<UnifiedAgent>) -> Self {
        self.agent = Some(agent);
        self
    }

    /// Set the model string (used if agent is not provided)
    pub fn with_model<S: Into<String>>(mut self, model: S) -> Self {
        self.model = Some(model.into());
        self
    }

    /// Set the system prompt
    pub fn with_system_prompt<S: Into<String>>(mut self, system_prompt: S) -> Self {
        self.system_prompt = Some(system_prompt.into());
        self
    }

    /// Add a retriever as a tool
    pub fn with_retriever(mut self, retriever: RetrieverInfo) -> Self {
        self.retrievers.push(retriever);
        self
    }

    /// Add multiple retrievers
    pub fn with_retrievers(mut self, retrievers: Vec<RetrieverInfo>) -> Self {
        self.retrievers.extend(retrievers);
        self
    }

    /// Add additional tools (non-retriever tools)
    pub fn with_tools(mut self, tools: &[Arc<dyn Tool>]) -> Self {
        self.additional_tools.extend_from_slice(tools);
        self
    }

    /// Build the AgenticRAG instance
    pub fn build(self) -> Result<AgenticRAG, RAGError> {
        // Create or use provided agent
        let agent = if let Some(agent) = self.agent {
            agent
        } else {
            let model = self.model.ok_or_else(|| {
                RAGError::InvalidConfiguration("Either agent or model must be set".to_string())
            })?;

            let system_prompt = self.system_prompt.unwrap_or_else(|| {
                "You are a helpful assistant. Use the retrieval tools when you need to find information from external knowledge sources.".to_string()
            });

            // Convert retrievers to tools
            let mut all_tools: Vec<Arc<dyn Tool>> = self.additional_tools;
            for retriever_info in &self.retrievers {
                let retriever_tool = Arc::new(
                    RetrieverTool::new(
                        retriever_info.retriever.clone(),
                        retriever_info.name.clone(),
                        retriever_info.description.clone(),
                    )
                    .with_max_docs(retriever_info.max_docs),
                );
                all_tools.push(retriever_tool);
            }

            Arc::new(
                crate::agent::create_agent(&model, &all_tools, Some(&system_prompt), None)
                    .map_err(|e| RAGError::ChainError(ChainError::AgentError(e.to_string())))?,
            )
        };

        // If agent was provided, we still need to add retriever tools
        // For now, we'll create the tools but note that they need to be added to the agent
        // In a full implementation, we'd need to rebuild the agent with the tools
        let mut retriever_tools: Vec<Arc<dyn Tool>> = Vec::new();
        for retriever_info in self.retrievers {
            let retriever_tool = Arc::new(
                RetrieverTool::new(
                    retriever_info.retriever,
                    retriever_info.name.clone(),
                    retriever_info.description,
                )
                .with_max_docs(retriever_info.max_docs),
            );
            retriever_tools.push(retriever_tool.clone());
        }

        Ok(AgenticRAG::new(agent, retriever_tools))
    }
}

impl Default for AgenticRAGBuilder {
    fn default() -> Self {
        Self::new()
    }
}
