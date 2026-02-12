use thiserror::Error;

#[derive(Error, Debug)]
pub enum MistralAIError {
    #[error("MistralAI API error: Invalid parameter - {0}")]
    InvalidParameterError(String),

    #[error("MistralAI API error: Invalid API Key - {0}")]
    InvalidApiKeyError(String),

    #[error("MistralAI API error: Network error - {0}")]
    NetworkError(String),

    #[error("MistralAI API error: Model Unavailable - {0}")]
    ModelUnavailableError(String),

    #[error("MistralAI API error: Rate limit exceeded - {0}")]
    RateLimitError(String),

    #[error("MistralAI API error: Internal error - {0}")]
    InternalError(String),

    #[error("MistralAI API error: System error - {0}")]
    SystemError(String),

    #[error("MistralAI API error: Client error - {0}")]
    ClientError(String),

    #[error("MistralAI API error: API error - {0}")]
    ApiError(String),
}
