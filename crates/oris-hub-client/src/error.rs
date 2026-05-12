#[derive(Debug, thiserror::Error)]
pub enum HubClientError {
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("hub returned error: {status} - {message}")]
    HubError { status: u16, message: String },

    #[error("serialization error: {0}")]
    Serialization(String),

    #[error("not registered")]
    NotRegistered,

    #[error("signing error: {0}")]
    Signing(String),
}
