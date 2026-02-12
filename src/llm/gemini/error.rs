use thiserror::Error;

#[derive(Error, Debug)]
pub enum GeminiError {
    #[error("Gemini API error: Invalid parameter - {0}")]
    InvalidParameterError(String),

    #[error("Gemini API error: Invalid API Key - {0}")]
    InvalidApiKeyError(String),

    #[error("Gemini API error: Network error - {0}")]
    NetworkError(String),

    #[error("Gemini API error: Model Unavailable - {0}")]
    ModelUnavailableError(String),

    #[error("Gemini API error: Rate limit exceeded - {0}")]
    RateLimitError(String),

    #[error("Gemini API error: Internal error - {0}")]
    InternalError(String),

    #[error("Gemini API error: System error - {0}")]
    SystemError(String),

    #[error("Gemini API error: Permission denied - {0}")]
    PermissionError(String),

    #[error("Gemini API error: Resource exhausted - {0}")]
    ResourceExhaustedError(String),
}
