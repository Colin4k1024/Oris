//! 错误处理工具函数
//!
//! 提供错误链追踪、上下文信息和错误代码等实用功能。

use std::fmt;

use super::LangChainError;

/// 错误代码系统
///
/// 为不同类型的错误分配唯一的错误代码，便于错误追踪和分类。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ErrorCode {
    /// LLM 相关错误 (1000-1999)
    LLMError = 1000,
    LLMTimeout = 1001,
    LLMRateLimit = 1002,
    LLMInvalidResponse = 1003,

    /// Chain 相关错误 (2000-2999)
    ChainError = 2000,
    ChainMissingInput = 2001,
    ChainInvalidInput = 2002,

    /// Agent 相关错误 (3000-3999)
    AgentError = 3000,
    AgentToolError = 3001,
    AgentMissingObject = 3002,
    AgentMaxIterations = 3003,

    /// RAG 相关错误 (4000-4999)
    RAGError = 4000,
    RAGRetrieverError = 4001,
    RAGQueryEnhancementError = 4002,
    RAGRetrievalValidationError = 4003,
    RAGAnswerValidationError = 4004,

    /// Multi-Agent 相关错误 (5000-5999)
    MultiAgentError = 5000,
    MultiAgentNotFound = 5001,
    MultiAgentRoutingError = 5002,
    MultiAgentSkillError = 5003,
    MultiAgentHandoffError = 5004,

    /// Vector Store 相关错误 (6000-6999)
    VectorStoreError = 6000,
    VectorStoreConnectionError = 6001,
    VectorStoreQueryError = 6002,

    /// Retriever 相关错误 (7000-7999)
    RetrieverError = 7000,
    RetrieverConnectionError = 7001,

    /// Tool 相关错误 (8000-8999)
    ToolError = 8000,
    ToolExecutionError = 8001,
    ToolValidationError = 8002,

    /// 通用错误 (9000-9999)
    ConfigurationError = 9000,
    IOError = 9001,
    JsonError = 9002,
    UnknownError = 9999,
}

impl ErrorCode {
    /// 从 LangChainError 获取错误代码
    pub fn from_error(error: &LangChainError) -> Self {
        match error {
            LangChainError::LLMError(_) => ErrorCode::LLMError,
            LangChainError::ChainError(_) => ErrorCode::ChainError,
            LangChainError::AgentError(_) => ErrorCode::AgentError,
            LangChainError::RAGError(_) => ErrorCode::RAGError,
            LangChainError::MultiAgentError(_) => ErrorCode::MultiAgentError,
            LangChainError::VectorStoreError(_) => ErrorCode::VectorStoreError,
            LangChainError::RetrieverError(_) => ErrorCode::RetrieverError,
            LangChainError::ToolError(_) => ErrorCode::ToolError,
            LangChainError::ConfigurationError(_) => ErrorCode::ConfigurationError,
            LangChainError::IOError(_) => ErrorCode::IOError,
            LangChainError::JsonError(_) => ErrorCode::JsonError,
            LangChainError::Unknown(_) => ErrorCode::UnknownError,
        }
    }

    /// 获取错误代码的数字值
    pub fn as_u32(self) -> u32 {
        self as u32
    }

    /// 获取错误代码的描述
    pub fn description(self) -> &'static str {
        match self {
            ErrorCode::LLMError => "LLM operation failed",
            ErrorCode::LLMTimeout => "LLM request timed out",
            ErrorCode::LLMRateLimit => "LLM rate limit exceeded",
            ErrorCode::LLMInvalidResponse => "LLM returned invalid response",
            ErrorCode::ChainError => "Chain operation failed",
            ErrorCode::ChainMissingInput => "Chain missing required input",
            ErrorCode::ChainInvalidInput => "Chain received invalid input",
            ErrorCode::AgentError => "Agent operation failed",
            ErrorCode::AgentToolError => "Agent tool execution failed",
            ErrorCode::AgentMissingObject => "Agent missing required object",
            ErrorCode::AgentMaxIterations => "Agent exceeded maximum iterations",
            ErrorCode::RAGError => "RAG operation failed",
            ErrorCode::RAGRetrieverError => "RAG retriever error",
            ErrorCode::RAGQueryEnhancementError => "RAG query enhancement failed",
            ErrorCode::RAGRetrievalValidationError => "RAG retrieval validation failed",
            ErrorCode::RAGAnswerValidationError => "RAG answer validation failed",
            ErrorCode::MultiAgentError => "Multi-agent operation failed",
            ErrorCode::MultiAgentNotFound => "Multi-agent not found",
            ErrorCode::MultiAgentRoutingError => "Multi-agent routing failed",
            ErrorCode::MultiAgentSkillError => "Multi-agent skill error",
            ErrorCode::MultiAgentHandoffError => "Multi-agent handoff failed",
            ErrorCode::VectorStoreError => "Vector store operation failed",
            ErrorCode::VectorStoreConnectionError => "Vector store connection failed",
            ErrorCode::VectorStoreQueryError => "Vector store query failed",
            ErrorCode::RetrieverError => "Retriever operation failed",
            ErrorCode::RetrieverConnectionError => "Retriever connection failed",
            ErrorCode::ToolError => "Tool operation failed",
            ErrorCode::ToolExecutionError => "Tool execution failed",
            ErrorCode::ToolValidationError => "Tool validation failed",
            ErrorCode::ConfigurationError => "Configuration error",
            ErrorCode::IOError => "IO operation failed",
            ErrorCode::JsonError => "JSON parsing/serialization failed",
            ErrorCode::UnknownError => "Unknown error",
        }
    }
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "E{:04}: {}", self.as_u32(), self.description())
    }
}

/// 错误上下文信息
///
/// 用于在错误发生时添加上下文信息，便于调试和追踪。
#[derive(Debug, Clone)]
pub struct ErrorContext {
    /// 操作名称
    pub operation: Option<String>,
    /// 模块名称
    pub module: Option<String>,
    /// 额外的上下文信息
    pub metadata: std::collections::HashMap<String, String>,
}

impl ErrorContext {
    /// 创建新的错误上下文
    pub fn new() -> Self {
        Self {
            operation: None,
            module: None,
            metadata: std::collections::HashMap::new(),
        }
    }

    /// 设置操作名称
    pub fn with_operation(mut self, operation: impl Into<String>) -> Self {
        self.operation = Some(operation.into());
        self
    }

    /// 设置模块名称
    pub fn with_module(mut self, module: impl Into<String>) -> Self {
        self.module = Some(module.into());
        self
    }

    /// 添加元数据
    pub fn with_metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }

    /// 格式化上下文信息
    pub fn format(&self) -> String {
        let mut parts = Vec::new();

        if let Some(ref module) = self.module {
            parts.push(format!("module: {}", module));
        }

        if let Some(ref operation) = self.operation {
            parts.push(format!("operation: {}", operation));
        }

        if !self.metadata.is_empty() {
            let metadata_str: Vec<String> = self
                .metadata
                .iter()
                .map(|(k, v)| format!("{}: {}", k, v))
                .collect();
            parts.push(format!("metadata: {}", metadata_str.join(", ")));
        }

        if parts.is_empty() {
            "no context".to_string()
        } else {
            parts.join("; ")
        }
    }
}

impl Default for ErrorContext {
    fn default() -> Self {
        Self::new()
    }
}

/// 为错误添加上下文信息
///
/// # 示例
///
/// ```rust,ignore
/// use oris::error::{ErrorContext, LangChainError};
///
/// let error = LangChainError::ConfigurationError("invalid config".to_string());
/// let context = ErrorContext::new()
///     .with_module("agent")
///     .with_operation("create_agent")
///     .with_metadata("model", "gpt-4");
///
/// let error_msg = format!("{} [{}]", error, context.format());
/// ```
pub fn error_context(error: &LangChainError) -> ErrorContext {
    let mut context = ErrorContext::new();

    match error {
        LangChainError::LLMError(_) => {
            context.module = Some("llm".to_string());
        }
        LangChainError::ChainError(_) => {
            context.module = Some("chain".to_string());
        }
        LangChainError::AgentError(_) => {
            context.module = Some("agent".to_string());
        }
        LangChainError::RAGError(_) => {
            context.module = Some("rag".to_string());
        }
        LangChainError::MultiAgentError(_) => {
            context.module = Some("multi_agent".to_string());
        }
        LangChainError::VectorStoreError(_) => {
            context.module = Some("vectorstore".to_string());
        }
        LangChainError::RetrieverError(_) => {
            context.module = Some("retriever".to_string());
        }
        LangChainError::ToolError(_) => {
            context.module = Some("tool".to_string());
        }
        _ => {}
    }

    context
}

/// 获取错误的完整信息，包括错误代码和上下文
///
/// # 示例
///
/// ```rust,ignore
/// use oris::error::{error_info, LangChainError};
///
/// let error = LangChainError::ConfigurationError("invalid config".to_string());
/// let info = error_info(&error);
/// println!("{}", info);
/// ```
pub fn error_info(error: &LangChainError) -> String {
    let code = ErrorCode::from_error(error);
    let context = error_context(error);

    format!("[{}] {} [{}]", code, error, context.format())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_code_from_error() {
        // Create a simple LLM error for testing
        let llm_error = crate::language_models::LLMError::OtherError("test".to_string());
        let error = LangChainError::LLMError(llm_error);
        let code = ErrorCode::from_error(&error);
        assert_eq!(code, ErrorCode::LLMError);
    }

    #[test]
    fn test_error_code_display() {
        let code = ErrorCode::LLMError;
        let display = format!("{}", code);
        assert!(display.contains("E1000"));
        assert!(display.contains("LLM operation failed"));
    }

    #[test]
    fn test_error_context() {
        let context = ErrorContext::new()
            .with_module("test")
            .with_operation("test_op")
            .with_metadata("key", "value");

        let formatted = context.format();
        assert!(formatted.contains("module: test"));
        assert!(formatted.contains("operation: test_op"));
        assert!(formatted.contains("key: value"));
    }

    #[test]
    fn test_error_info() {
        let error = LangChainError::ConfigurationError("test".to_string());
        let info = error_info(&error);
        assert!(info.contains("E9000"));
        assert!(info.contains("test"));
    }
}
