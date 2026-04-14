//! OEN error types.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum OenError {
    #[error("invalid envelope format")]
    InvalidEnvelope,

    #[error("invalid message type: expected {expected}, got {actual}")]
    InvalidMessageType { expected: String, actual: String },

    #[error("sender_id mismatch: expected {expected}, got {actual}")]
    SenderMismatch { expected: String, actual: String },

    #[error("timestamp expired: envelope is {seconds}s old, max allowed is {max}s")]
    TimestampExpired { seconds: i64, max: i64 },

    #[error("signature verification failed")]
    SignatureFailed,

    #[error("missing signature")]
    MissingSignature,

    #[error("content hash mismatch")]
    ContentHashMismatch,

    #[error("signing error: {0}")]
    SigningError(String),

    #[error("envelope parsing error: {0}")]
    ParseError(String),
}
