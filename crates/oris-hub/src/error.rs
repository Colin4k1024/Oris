use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

#[derive(Debug, thiserror::Error)]
pub enum HubError {
    #[error("node not found: {0}")]
    NodeNotFound(String),

    #[error("invalid signature: {0}")]
    InvalidSignature(String),

    #[error("registration failed: {0}")]
    RegistrationFailed(String),

    #[error("storage error: {0}")]
    Storage(String),

    #[error("rate limited")]
    RateLimited,

    #[error("federation error: {0}")]
    Federation(String),

    #[error("subscription error: {0}")]
    Subscription(String),

    #[error("unauthorized: {0}")]
    Unauthorized(String),

    #[error("validation error: {0}")]
    Validation(String),

    #[error("conflict: {0}")]
    Conflict(String),

    #[error("internal error: {0}")]
    Internal(String),
}

impl From<rusqlite::Error> for HubError {
    fn from(e: rusqlite::Error) -> Self {
        HubError::Storage(e.to_string())
    }
}

impl IntoResponse for HubError {
    fn into_response(self) -> Response {
        let (status, message) = match &self {
            HubError::NodeNotFound(_) => (StatusCode::NOT_FOUND, self.to_string()),
            HubError::InvalidSignature(_) => (StatusCode::UNAUTHORIZED, self.to_string()),
            HubError::Unauthorized(_) => (StatusCode::UNAUTHORIZED, self.to_string()),
            HubError::RateLimited => (StatusCode::TOO_MANY_REQUESTS, self.to_string()),
            HubError::Validation(_) => (StatusCode::BAD_REQUEST, self.to_string()),
            HubError::Conflict(_) => (StatusCode::CONFLICT, self.to_string()),
            HubError::RegistrationFailed(_) => (StatusCode::BAD_REQUEST, self.to_string()),
            HubError::Subscription(_) => (StatusCode::BAD_REQUEST, self.to_string()),
            HubError::Federation(_) => (StatusCode::BAD_GATEWAY, self.to_string()),
            HubError::Storage(_) | HubError::Internal(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal error".to_string(),
            ),
        };

        let body = serde_json::json!({ "error": message });
        (status, axum::Json(body)).into_response()
    }
}
