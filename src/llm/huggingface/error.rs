use thiserror::Error;

#[derive(Error, Debug)]
pub enum HuggingFaceError {
    #[error("HuggingFace API error: Invalid parameter - {0}")]
    InvalidParameterError(String),

    #[error("HuggingFace API error: Invalid API Key - {0}")]
    InvalidApiKeyError(String),

    #[error("HuggingFace API error: Network error - {0}")]
    NetworkError(String),

    #[error("HuggingFace API error: Model Unavailable - {0}")]
    ModelUnavailableError(String),

    #[error("HuggingFace API error: Rate limit exceeded - {0}")]
    RateLimitError(String),

    #[error("HuggingFace API error: Internal error - {0}")]
    InternalError(String),

    #[error("HuggingFace API error: System error - {0}")]
    SystemError(String),

    #[error("HuggingFace API error: Model not found - {0}")]
    ModelNotFoundError(String),
}
