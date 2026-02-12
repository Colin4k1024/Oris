use thiserror::Error;

#[derive(Error, Debug)]
pub enum BedrockError {
    #[error("Bedrock API error: Invalid parameter - {0}")]
    InvalidParameterError(String),

    #[error("Bedrock API error: Authentication error - {0}")]
    AuthenticationError(String),

    #[error("Bedrock API error: Network error - {0}")]
    NetworkError(String),

    #[error("Bedrock API error: Model Unavailable - {0}")]
    ModelUnavailableError(String),

    #[error("Bedrock API error: Rate limit exceeded - {0}")]
    RateLimitError(String),

    #[error("Bedrock API error: Internal error - {0}")]
    InternalError(String),

    #[error("Bedrock API error: System error - {0}")]
    SystemError(String),

    #[error("Bedrock API error: Throttling - {0}")]
    ThrottlingError(String),

    #[error("Bedrock API error: Validation error - {0}")]
    ValidationError(String),
}
