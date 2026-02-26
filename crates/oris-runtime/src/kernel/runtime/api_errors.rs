//! HTTP error mapping for execution server handlers.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Serialize;
use serde_json::Value;

#[derive(Clone, Debug, Serialize)]
pub struct ErrorBody {
    pub code: &'static str,
    pub message: String,
    pub details: Option<Value>,
}

#[derive(Debug)]
pub enum ApiError {
    BadRequest(ErrorState),
    NotFound(ErrorState),
    Conflict(ErrorState),
    Internal(ErrorState),
}

#[derive(Clone, Debug)]
pub struct ErrorState {
    pub message: String,
    pub request_id: Option<String>,
    pub details: Option<Value>,
}

impl ErrorState {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            request_id: None,
            details: None,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
struct ErrorEnvelope {
    request_id: String,
    error: ErrorBody,
}

impl ApiError {
    pub fn bad_request(message: impl Into<String>) -> Self {
        Self::BadRequest(ErrorState::new(message))
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::NotFound(ErrorState::new(message))
    }

    pub fn conflict(message: impl Into<String>) -> Self {
        Self::Conflict(ErrorState::new(message))
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::Internal(ErrorState::new(message))
    }

    pub fn with_request_id(mut self, request_id: impl Into<String>) -> Self {
        let request_id = Some(request_id.into());
        match &mut self {
            Self::BadRequest(s) | Self::NotFound(s) | Self::Conflict(s) | Self::Internal(s) => {
                s.request_id = request_id
            }
        }
        self
    }

    pub fn with_details(mut self, details: Value) -> Self {
        match &mut self {
            Self::BadRequest(s) | Self::NotFound(s) | Self::Conflict(s) | Self::Internal(s) => {
                s.details = Some(details)
            }
        }
        self
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, code, state) = match self {
            Self::BadRequest(s) => (StatusCode::BAD_REQUEST, "invalid_argument", s),
            Self::NotFound(s) => (StatusCode::NOT_FOUND, "not_found", s),
            Self::Conflict(s) => (StatusCode::CONFLICT, "conflict", s),
            Self::Internal(s) => (StatusCode::INTERNAL_SERVER_ERROR, "internal", s),
        };
        let request_id = state
            .request_id
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let body = ErrorEnvelope {
            request_id,
            error: ErrorBody {
                code,
                message: state.message,
                details: state.details,
            },
        };
        (status, Json(body)).into_response()
    }
}
