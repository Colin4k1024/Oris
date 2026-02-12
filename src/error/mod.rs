//! 统一的错误处理模块
//!
//! 提供 oris 项目所有模块的错误类型定义。
//! 使用 thiserror 库，提供类型安全且易于理解的错误类型。

pub use crate::chain::ChainError;
pub use crate::language_models::LLMError;
pub use crate::retrievers::RetrieverError;
pub use crate::tools::ToolError;
pub use crate::vectorstore::VectorStoreError;

// Re-export agent-related errors for convenience
pub use crate::agent::AgentError;
pub use crate::rag::RAGError;
// Note: MultiAgentError is re-exported through agent module

// Re-export error utilities
pub mod utils;
pub use utils::{error_context, error_info, ErrorCode, ErrorContext};

/// 统一的错误枚举，组合所有子模块错误
///
/// 这个枚举作为整个项目的顶层错误类型，
/// 允许所有子模块的错误向上传播。
///
/// # 使用示例
///
/// ```rust,ignore
/// use oris::error::LangChainError;
///
/// async fn example() -> Result<(), LangChainError> {
///     // 所有子模块错误都可以自动转换
///     some_agent_operation().await?;
///     some_rag_operation().await?;
///     Ok(())
/// }
/// ```
#[derive(thiserror::Error, Debug)]
pub enum LangChainError {
    #[error("LLM error: {0}")]
    LLMError(#[from] LLMError),

    #[error("Chain error: {0}")]
    ChainError(#[from] ChainError),

    #[error("Vector store error: {0}")]
    VectorStoreError(#[from] VectorStoreError),

    #[error("Retriever error: {0}")]
    RetrieverError(#[from] RetrieverError),

    #[error("Tool error: {0}")]
    ToolError(#[from] ToolError),

    #[error("Agent error: {0}")]
    AgentError(#[from] crate::agent::AgentError),

    #[error("RAG error: {0}")]
    RAGError(#[from] crate::rag::RAGError),

    #[error("Multi-agent error: {0}")]
    MultiAgentError(#[from] crate::agent::MultiAgentError),

    #[error("Configuration error: {0}")]
    ConfigurationError(String),

    #[error("IO error: {0}")]
    IOError(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    JsonError(#[from] serde_json::Error),

    #[error("Unknown error: {0}")]
    Unknown(String),
}

// 便利的类型别名
pub type Result<T> = std::result::Result<T, LangChainError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_langchain_error_creation() {
        let chain_error = ChainError::OtherError("test".to_string());
        let langchain_error: LangChainError = chain_error.into();

        match langchain_error {
            LangChainError::ChainError(_) => {}
            _ => panic!("Expected ChainError variant"),
        }
    }

    #[test]
    fn test_tool_error_creation() {
        let tool_error = ToolError::ExecutionError("test execution".to_string());
        let langchain_error: LangChainError = tool_error.into();

        match langchain_error {
            LangChainError::ToolError(_) => {}
            _ => panic!("Expected ToolError variant"),
        }
    }

    #[test]
    fn test_vectorstore_error_creation() {
        let vectorstore_error = VectorStoreError::DeleteNotSupported;
        let langchain_error: LangChainError = vectorstore_error.into();

        match langchain_error {
            LangChainError::VectorStoreError(_) => {}
            _ => panic!("Expected VectorStoreError variant"),
        }
    }

    #[test]
    fn test_retriever_error_creation() {
        let retriever_error = RetrieverError::WikipediaError("test wikipedia".to_string());
        let langchain_error: LangChainError = retriever_error.into();

        match langchain_error {
            LangChainError::RetrieverError(_) => {}
            _ => panic!("Expected RetrieverError variant"),
        }
    }

    #[test]
    fn test_agent_error_creation() {
        let agent_error = crate::agent::AgentError::OtherError("test agent".to_string());
        let langchain_error: LangChainError = agent_error.into();

        match langchain_error {
            LangChainError::AgentError(_) => {}
            _ => panic!("Expected AgentError variant"),
        }
    }

    #[test]
    fn test_rag_error_creation() {
        let rag_error = crate::rag::RAGError::InvalidConfiguration("test config".to_string());
        let langchain_error: LangChainError = rag_error.into();

        match langchain_error {
            LangChainError::RAGError(_) => {}
            _ => panic!("Expected RAGError variant"),
        }
    }

    #[test]
    fn test_multi_agent_error_creation() {
        let multi_agent_error =
            crate::agent::multi_agent::MultiAgentError::AgentNotFound("test_agent".to_string());
        let langchain_error: LangChainError = multi_agent_error.into();

        match langchain_error {
            LangChainError::MultiAgentError(_) => {}
            _ => panic!("Expected MultiAgentError variant"),
        }
    }
}
