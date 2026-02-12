use std::error::Error;
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;

use crate::{
    schemas::{Document, Retriever},
    tools::{Tool, ToolResult, ToolRuntime},
};

/// A tool that wraps a Retriever, allowing agents to retrieve documents on demand.
///
/// This enables the Agentic RAG pattern where an agent decides when and how
/// to retrieve information during reasoning.
pub struct RetrieverTool {
    /// The retriever to use
    retriever: Arc<dyn Retriever>,
    /// Name of the tool
    name: String,
    /// Description of what this tool does
    description: String,
    /// Maximum number of documents to retrieve
    max_docs: usize,
}

impl RetrieverTool {
    /// Create a new RetrieverTool
    pub fn new(retriever: Arc<dyn Retriever>, name: String, description: String) -> Self {
        Self {
            retriever,
            name,
            description,
            max_docs: 5,
        }
    }

    /// Set the maximum number of documents to retrieve
    pub fn with_max_docs(mut self, max_docs: usize) -> Self {
        self.max_docs = max_docs;
        self
    }

    /// Get a reference to the wrapped retriever
    pub fn retriever(&self) -> &Arc<dyn Retriever> {
        &self.retriever
    }
}

#[async_trait]
impl Tool for RetrieverTool {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn description(&self) -> String {
        self.description.clone()
    }

    fn parameters(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": format!("The search query to retrieve relevant documents. {}", self.description)
                }
            },
            "required": ["query"]
        })
    }

    async fn run(&self, _input: Value) -> Result<String, crate::error::ToolError> {
        // This method is not used when requires_runtime() returns true
        Err(crate::error::ToolError::ConfigurationError(
            "RetrieverTool requires runtime. Use run_with_runtime instead.".to_string(),
        ))
    }

    async fn run_with_runtime(
        &self,
        input: Value,
        _runtime: &ToolRuntime,
    ) -> Result<ToolResult, Box<dyn Error>> {
        // Extract query from input
        let query = if let Some(query_val) = input.get("query") {
            if query_val.is_string() {
                query_val.as_str().unwrap().to_string()
            } else {
                return Err("query must be a string".into());
            }
        } else if input.is_string() {
            input.as_str().unwrap().to_string()
        } else {
            return Err("query is required".into());
        };

        // Retrieve documents
        let documents = self.retriever.get_relevant_documents(&query).await?;

        // Limit the number of documents
        let limited_docs: Vec<&Document> = documents.iter().take(self.max_docs).collect();

        // Format the documents as a string
        let result = if limited_docs.is_empty() {
            format!("No relevant documents found for query: {}", query)
        } else {
            let doc_strings: Vec<String> = limited_docs
                .iter()
                .enumerate()
                .map(|(i, doc)| {
                    format!(
                        "[Document {}]\nSource: {:?}\nContent: {}\n",
                        i + 1,
                        doc.metadata
                            .get("source")
                            .and_then(|v| v.as_str())
                            .unwrap_or("unknown"),
                        doc.page_content
                    )
                })
                .collect();
            format!(
                "Retrieved {} document(s) for query '{}':\n\n{}",
                limited_docs.len(),
                query,
                doc_strings.join("\n---\n\n")
            )
        };

        Ok(ToolResult::Text(result))
    }

    fn requires_runtime(&self) -> bool {
        true
    }
}
