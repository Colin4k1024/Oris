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

    #[error("query parse error: {0}")]
    QueryParseError(String),

    #[error("gene store error: {0}")]
    GeneStoreError(#[from] anyhow::Error),

    #[error("internal error: {0}")]
    InternalError(String),
}

impl IntoResponse for ExperienceRepoError {
    fn into_response(self) -> Response {
        let (status, message) = match &self {
            ExperienceRepoError::ApiKeyMissing => (StatusCode::UNAUTHORIZED, self.to_string()),
            ExperienceRepoError::InvalidApiKey => (StatusCode::UNAUTHORIZED, self.to_string()),
            ExperienceRepoError::QueryParseError(_) => (StatusCode::BAD_REQUEST, self.to_string()),
            ExperienceRepoError::GeneStoreError(_) => (StatusCode::INTERNAL_SERVER_ERROR, "gene store error".to_string()),
            ExperienceRepoError::InternalError(_) => (StatusCode::INTERNAL_SERVER_ERROR, self.to_string()),
        };

        let body = serde_json::json!({
            "error": message
        });

        Response::builder()
            .status(status)
            .header("Content-Type", "application/json")
            .body(axum::body::Body::from(body.to_string()))
            .unwrap()
    }
}
