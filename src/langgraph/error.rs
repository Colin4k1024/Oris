use thiserror::Error;

/// Errors that can occur when working with LangGraph
#[derive(Error, Debug)]
pub enum LangGraphError {
    #[error("Node not found: {0}")]
    NodeNotFound(String),

    #[error("Invalid edge: from '{0}' to '{1}'")]
    InvalidEdge(String, String),

    #[error("Execution error: {0}")]
    ExecutionError(String),

    #[error("Graph compilation error: {0}")]
    CompilationError(String),

    #[error("State merge error: {0}")]
    StateMergeError(String),

    #[error("Condition function error: {0}")]
    ConditionError(String),

    #[error("Circular dependency detected")]
    CircularDependency,

    #[error("No path from START to END")]
    NoPathToEnd,

    #[error("Invalid state update: {0}")]
    InvalidStateUpdate(String),

    #[error("Streaming error: {0}")]
    StreamingError(String),

    #[error("Chain error: {0}")]
    ChainError(#[from] crate::chain::ChainError),

    #[error("LLM error: {0}")]
    LLMError(String),

    #[error("Agent error: {0}")]
    AgentError(#[from] crate::agent::AgentError),

    #[error("Serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),

    #[error("Interrupt error: {0}")]
    InterruptError(#[from] super::interrupts::error::InterruptError),
}

impl From<crate::language_models::LLMError> for LangGraphError {
    fn from(e: crate::language_models::LLMError) -> Self {
        LangGraphError::LLMError(e.to_string())
    }
}

pub type LangGraphResult<T> = Result<T, LangGraphError>;
