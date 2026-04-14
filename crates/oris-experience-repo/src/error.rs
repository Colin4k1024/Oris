//! Error types for Experience Repository.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ExperienceRepoError {
    #[error("api key missing")]
    ApiKeyMissing,

    #[error("invalid api key")]
    InvalidApiKey,

    #[error("api key expired")]
    KeyExpired,

    #[error("api key revoked")]
    KeyRevoked,

    #[error("agent_id mismatch")]
    AgentMismatch,

    #[error("invalid envelope")]
    InvalidEnvelope,

    #[error("invalid signature")]
    InvalidSignature,

    #[error("timestamp expired")]
    TimestampExpired,

    #[error("sender mismatch")]
    SenderMismatch,

    #[error("public key not found")]
    PublicKeyNotFound,

    #[error("invalid public key")]
    InvalidPublicKey,

    #[error("rate limit exceeded")]
    RateLimitExceeded(u64),

    #[error("query parse error: {0}")]
    QueryParseError(String),

    #[error("gene store error: {0}")]
    GeneStoreError(#[from] anyhow::Error),

    #[error("key service error: {0}")]
    KeyServiceError(String),

    #[error("oen error: {0}")]
    OenError(String),

    #[error("duplicate gene")]
    DuplicateGene,

    #[error("internal error: {0}")]
    InternalError(String),
}

impl From<crate::key_service::KeyServiceError> for ExperienceRepoError {
    fn from(err: crate::key_service::KeyServiceError) -> Self {
        match err {
            crate::key_service::KeyServiceError::KeyNotFound => ExperienceRepoError::InvalidApiKey,
            crate::key_service::KeyServiceError::InvalidKey => ExperienceRepoError::InvalidApiKey,
            crate::key_service::KeyServiceError::Expired => ExperienceRepoError::KeyExpired,
            crate::key_service::KeyServiceError::Revoked => ExperienceRepoError::KeyRevoked,
            crate::key_service::KeyServiceError::AgentMismatch => {
                ExperienceRepoError::AgentMismatch
            }
            crate::key_service::KeyServiceError::KeyAlreadyExists => {
                ExperienceRepoError::InternalError("key already exists".to_string())
            }
            crate::key_service::KeyServiceError::StoreError(e) => {
                ExperienceRepoError::KeyServiceError(e)
            }
            crate::key_service::KeyServiceError::PublicKeyNotFound => {
                ExperienceRepoError::PublicKeyNotFound
            }
            crate::key_service::KeyServiceError::InvalidPublicKey => {
                ExperienceRepoError::InvalidPublicKey
            }
        }
    }
}

impl From<crate::oen::OenError> for ExperienceRepoError {
    fn from(err: crate::oen::OenError) -> Self {
        match err {
            crate::oen::OenError::InvalidEnvelope => ExperienceRepoError::InvalidEnvelope,
            crate::oen::OenError::InvalidMessageType { .. } => ExperienceRepoError::InvalidEnvelope,
            crate::oen::OenError::SenderMismatch { .. } => ExperienceRepoError::SenderMismatch,
            crate::oen::OenError::TimestampExpired { .. } => ExperienceRepoError::TimestampExpired,
            crate::oen::OenError::SignatureFailed => ExperienceRepoError::InvalidSignature,
            crate::oen::OenError::MissingSignature => ExperienceRepoError::InvalidSignature,
            crate::oen::OenError::ContentHashMismatch => ExperienceRepoError::InvalidSignature,
            crate::oen::OenError::SigningError(_) => {
                ExperienceRepoError::InternalError("signing error".to_string())
            }
            crate::oen::OenError::ParseError(_) => ExperienceRepoError::InvalidEnvelope,
        }
    }
}

impl IntoResponse for ExperienceRepoError {
    fn into_response(self) -> Response {
        let (status, error_code, message) = match &self {
            ExperienceRepoError::ApiKeyMissing => (
                StatusCode::UNAUTHORIZED,
                "API_KEY_MISSING",
                self.to_string(),
            ),
            ExperienceRepoError::InvalidApiKey => (
                StatusCode::UNAUTHORIZED,
                "INVALID_API_KEY",
                self.to_string(),
            ),
            ExperienceRepoError::KeyExpired => {
                (StatusCode::UNAUTHORIZED, "KEY_EXPIRED", self.to_string())
            }
            ExperienceRepoError::KeyRevoked => {
                (StatusCode::UNAUTHORIZED, "KEY_REVOKED", self.to_string())
            }
            ExperienceRepoError::AgentMismatch => {
                (StatusCode::FORBIDDEN, "AGENT_MISMATCH", self.to_string())
            }
            ExperienceRepoError::InvalidEnvelope => (
                StatusCode::BAD_REQUEST,
                "INVALID_ENVELOPE",
                self.to_string(),
            ),
            ExperienceRepoError::InvalidSignature => {
                (StatusCode::FORBIDDEN, "INVALID_SIGNATURE", self.to_string())
            }
            ExperienceRepoError::TimestampExpired => {
                (StatusCode::FORBIDDEN, "TIMESTAMP_EXPIRED", self.to_string())
            }
            ExperienceRepoError::SenderMismatch => {
                (StatusCode::FORBIDDEN, "SENDER_MISMATCH", self.to_string())
            }
            ExperienceRepoError::PublicKeyNotFound => (
                StatusCode::NOT_FOUND,
                "PUBLIC_KEY_NOT_FOUND",
                self.to_string(),
            ),
            ExperienceRepoError::InvalidPublicKey => (
                StatusCode::BAD_REQUEST,
                "INVALID_PUBLIC_KEY",
                self.to_string(),
            ),
            ExperienceRepoError::RateLimitExceeded(retry_after) => (
                StatusCode::TOO_MANY_REQUESTS,
                "RATE_LIMIT_EXCEEDED",
                format!("rate limit exceeded, retry after {} seconds", retry_after),
            ),
            ExperienceRepoError::QueryParseError(_) => {
                (StatusCode::BAD_REQUEST, "QUERY_ERROR", self.to_string())
            }
            ExperienceRepoError::DuplicateGene => {
                (StatusCode::CONFLICT, "DUPLICATE_GENE", self.to_string())
            }
            ExperienceRepoError::GeneStoreError(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "GENE_STORE_ERROR",
                "gene store error".to_string(),
            ),
            ExperienceRepoError::KeyServiceError(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "KEY_SERVICE_ERROR",
                "key service error".to_string(),
            ),
            ExperienceRepoError::OenError(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "OEN_ERROR",
                "envelope processing error".to_string(),
            ),
            ExperienceRepoError::InternalError(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "INTERNAL_ERROR",
                self.to_string(),
            ),
        };

        let body = serde_json::json!({
            "error": message,
            "error_code": error_code
        });

        Response::builder()
            .status(status)
            .header("Content-Type", "application/json")
            .body(axum::body::Body::from(body.to_string()))
            .unwrap()
    }
}
