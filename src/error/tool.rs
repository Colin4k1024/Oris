use thiserror::Error;

/// Tool 相关的所有错误类型
#[derive(Error, Debug)]
pub enum ToolError {
    // ============ 执行错误 ============
    #[error("Execution failed: {0}")]
    ExecutionError(String),

    #[error("Tool not found: {0}")]
    ToolNotFound(String),

    // ============ 输入验证错误 ============
    #[error("Invalid input: {0}")]
    InvalidInputError(String),

    #[error("Input parsing failed: {0}")]
    ParsingError(String),

    #[error("Missing required input: {0}")]
    MissingInput(String),

    // ============ 资源错误 ============
    #[error("Timeout: {0}")]
    TimeoutError(String),

    #[error("Rate limit exceeded")]
    RateLimitError,

    #[error("Resource not found: {0}")]
    ResourceNotFound(String),

    // ============ 权限和安全错误 ============
    #[error("Permission denied: {0}")]
    PermissionError(String),

    #[error("Security error: {0}")]
    SecurityError(String),

    // ============ 配置错误 ============
    #[error("Configuration error: {0}")]
    ConfigurationError(String),

    #[error("Missing configuration: {0}")]
    MissingConfiguration(String),

    // ============ 外部服务错误 ============
    #[error("External service error: {0}")]
    ExternalServiceError(String),

    #[error("Network error: {0}")]
    NetworkError(String),

    // ============ 内部错误 ============
    #[error("Internal error: {0}")]
    InternalError(String),

    #[error("Unknown error: {0}")]
    Unknown(String),
}

impl From<String> for ToolError {
    fn from(s: String) -> Self {
        ToolError::Unknown(s)
    }
}
