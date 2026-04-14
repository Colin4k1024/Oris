//! Key Service error types.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum KeyServiceError {
    #[error("key not found")]
    KeyNotFound,

    #[error("invalid api key")]
    InvalidKey,

    #[error("key expired")]
    Expired,

    #[error("key revoked")]
    Revoked,

    #[error("agent_id mismatch")]
    AgentMismatch,

    #[error("key already exists")]
    KeyAlreadyExists,

    #[error("store error: {0}")]
    StoreError(String),

    #[error("public key not found")]
    PublicKeyNotFound,

    #[error("invalid public key format")]
    InvalidPublicKey,
}

impl From<rusqlite::Error> for KeyServiceError {
    fn from(err: rusqlite::Error) -> Self {
        KeyServiceError::StoreError(err.to_string())
    }
}
