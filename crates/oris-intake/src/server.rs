//! Axum webhook HTTP server for the Oris Intake system.
//!
//! Exposes four endpoints:
//!
//! | Method | Path                   | Source                  |
//! |--------|------------------------|-------------------------|
//! | POST   | `/webhooks/github`     | GitHub Actions / webhook |
//! | POST   | `/webhooks/gitlab`     | GitLab CI               |
//! | POST   | `/webhooks/prometheus` | Alertmanager v4         |
//! | POST   | `/webhooks/sentry`     | Sentry issue alerts     |
//!
//! Enable with the `webhook` feature flag.
//!
//! ## Security
//!
//! - **GitHub**: `X-Hub-Signature-256: sha256=<hmac>` header is verified with
//!   HMAC-SHA256 if a `github_secret` is configured. Invalid/missing signatures
//!   are rejected with `403 Forbidden`.
//! - **GitLab**: `X-Gitlab-Token` header is compared in constant time against
//!   the configured `gitlab_token`. Missing/mismatched tokens are rejected with
//!   `403 Forbidden`.
//! - Prometheus and Sentry endpoints do not require signature verification (they
//!   are typically protected at the network level), but callers may add their own
//!   middleware via Axum's layer system.

use std::sync::Arc;

use axum::{
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::post,
    Router,
};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use tokio::sync::mpsc;

use crate::{
    source::{
        from_gitlab_pipeline, GithubIntakeSource, GitlabPipelineEvent, IntakeSource,
        PrometheusIntakeSource, SentryIntakeSource,
    },
    IntakeEvent,
};

// ---------------------------------------------------------------------------
// Server configuration
// ---------------------------------------------------------------------------

/// Shared state injected into each Axum handler.
#[derive(Clone)]
pub struct WebhookState {
    /// Sender side of the event channel. Events are sent here after parsing.
    pub tx: mpsc::Sender<IntakeEvent>,
    /// Optional HMAC-SHA256 secret for GitHub webhook signature verification.
    /// When `None`, signature verification is **skipped** for the GitHub endpoint.
    pub github_secret: Option<String>,
    /// Optional shared token for GitLab webhook token verification.
    /// When `None`, token verification is **skipped** for the GitLab endpoint.
    pub gitlab_token: Option<String>,
}

/// Builder for the webhook server.
pub struct WebhookServer {
    state: WebhookState,
}

impl WebhookServer {
    /// Create a new server.
    ///
    /// `tx` is the channel on which parsed `IntakeEvent`s will be sent.
    /// Configure secrets / tokens via the builder methods before calling
    /// [`WebhookServer::into_router`].
    pub fn new(tx: mpsc::Sender<IntakeEvent>) -> Self {
        Self {
            state: WebhookState {
                tx,
                github_secret: None,
                gitlab_token: None,
            },
        }
    }

    /// Set the HMAC-SHA256 secret for GitHub webhook signature verification.
    pub fn with_github_secret(mut self, secret: impl Into<String>) -> Self {
        self.state.github_secret = Some(secret.into());
        self
    }

    /// Set the shared token for GitLab webhook token verification.
    pub fn with_gitlab_token(mut self, token: impl Into<String>) -> Self {
        self.state.gitlab_token = Some(token.into());
        self
    }

    /// Build an Axum [`Router`] with all four webhook routes mounted.
    pub fn into_router(self) -> Router {
        let shared = Arc::new(self.state);
        Router::new()
            .route("/webhooks/github", post(handle_github))
            .route("/webhooks/gitlab", post(handle_gitlab))
            .route("/webhooks/prometheus", post(handle_prometheus))
            .route("/webhooks/sentry", post(handle_sentry))
            .with_state(shared)
    }
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// `POST /webhooks/github`
async fn handle_github(
    State(state): State<Arc<WebhookState>>,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    // Signature verification
    if let Some(secret) = &state.github_secret {
        let sig_header = headers
            .get("x-hub-signature-256")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");

        if !verify_github_signature(secret.as_bytes(), &body, sig_header) {
            return StatusCode::FORBIDDEN.into_response();
        }
    }

    let source = GithubIntakeSource::auto();
    match source.process(&body) {
        Ok(events) => {
            emit_all(&state.tx, events).await;
            StatusCode::OK.into_response()
        }
        Err(e) => (StatusCode::BAD_REQUEST, format!("parse error: {}", e)).into_response(),
    }
}

/// `POST /webhooks/gitlab`
async fn handle_gitlab(
    State(state): State<Arc<WebhookState>>,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    // Token verification
    if let Some(expected) = &state.gitlab_token {
        let provided = headers
            .get("x-gitlab-token")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");

        if !constant_time_eq(expected.as_bytes(), provided.as_bytes()) {
            return StatusCode::FORBIDDEN.into_response();
        }
    }

    let event: GitlabPipelineEvent = match serde_json::from_slice(&body) {
        Ok(e) => e,
        Err(e) => {
            return (StatusCode::BAD_REQUEST, format!("parse error: {}", e)).into_response();
        }
    };

    match from_gitlab_pipeline(event) {
        Ok(intake_event) => {
            emit_all(&state.tx, vec![intake_event]).await;
            StatusCode::OK.into_response()
        }
        Err(e) => (StatusCode::BAD_REQUEST, format!("conversion error: {}", e)).into_response(),
    }
}

/// `POST /webhooks/prometheus`
async fn handle_prometheus(
    State(state): State<Arc<WebhookState>>,
    body: Bytes,
) -> impl IntoResponse {
    let source = PrometheusIntakeSource;
    match source.process(&body) {
        Ok(events) => {
            emit_all(&state.tx, events).await;
            StatusCode::OK.into_response()
        }
        Err(e) => (StatusCode::BAD_REQUEST, format!("parse error: {}", e)).into_response(),
    }
}

/// `POST /webhooks/sentry`
async fn handle_sentry(State(state): State<Arc<WebhookState>>, body: Bytes) -> impl IntoResponse {
    let source = SentryIntakeSource;
    match source.process(&body) {
        Ok(events) => {
            emit_all(&state.tx, events).await;
            StatusCode::OK.into_response()
        }
        Err(e) => (StatusCode::BAD_REQUEST, format!("parse error: {}", e)).into_response(),
    }
}

// ---------------------------------------------------------------------------
// Security helpers
// ---------------------------------------------------------------------------

/// Verify a GitHub `X-Hub-Signature-256` header value.
///
/// `sig_header` must be in the form `sha256=<hex>`.
/// Returns `false` if the header is missing, malformed, or the HMAC does not match.
fn verify_github_signature(secret: &[u8], body: &[u8], sig_header: &str) -> bool {
    let hex_sig = match sig_header.strip_prefix("sha256=") {
        Some(h) => h,
        None => return false,
    };

    let expected = match hex::decode(hex_sig) {
        Ok(b) => b,
        Err(_) => return false,
    };

    let mut mac = Hmac::<Sha256>::new_from_slice(secret).expect("HMAC accepts any key length");
    mac.update(body);
    let computed = mac.finalize().into_bytes();

    constant_time_eq(&computed, &expected)
}

/// Constant-time byte slice comparison to prevent timing attacks.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter()
        .zip(b.iter())
        .fold(0u8, |acc, (x, y)| acc | (x ^ y))
        == 0
}

// ---------------------------------------------------------------------------
// Channel helper
// ---------------------------------------------------------------------------

async fn emit_all(tx: &mpsc::Sender<IntakeEvent>, events: Vec<IntakeEvent>) {
    for event in events {
        // Best-effort send; a full or closed channel is not a fatal error for
        // the HTTP handler (the caller receives 200 regardless).
        let _ = tx.try_send(event);
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Body,
        http::{Method, Request, StatusCode},
    };
    use hmac::{Hmac, Mac};
    use sha2::Sha256;
    use tokio::sync::mpsc;
    use tower::util::ServiceExt; // for `.oneshot()`

    fn make_app(secret: Option<&str>, gitlab_token: Option<&str>) -> Router {
        let (tx, _rx) = mpsc::channel(64);
        let mut builder = WebhookServer::new(tx);
        if let Some(s) = secret {
            builder = builder.with_github_secret(s);
        }
        if let Some(t) = gitlab_token {
            builder = builder.with_gitlab_token(t);
        }
        builder.into_router()
    }

    fn github_sig(secret: &str, body: &[u8]) -> String {
        let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).expect("hmac key");
        mac.update(body);
        format!("sha256={}", hex::encode(mac.finalize().into_bytes()))
    }

    // -----------------------------------------------------------------------
    // GitHub endpoint
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn github_missing_signature_returns_403() {
        let app = make_app(Some("mysecret"), None);
        let body = b"{}";
        let req = Request::builder()
            .method(Method::POST)
            .uri("/webhooks/github")
            .header("content-type", "application/json")
            .body(Body::from(body.as_slice()))
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn github_invalid_signature_returns_403() {
        let app = make_app(Some("mysecret"), None);
        let body = b"{}";
        let req = Request::builder()
            .method(Method::POST)
            .uri("/webhooks/github")
            .header("content-type", "application/json")
            .header("x-hub-signature-256", "sha256=deadbeef")
            .body(Body::from(body.as_slice()))
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn github_valid_signature_returns_200() {
        let secret = "mysecret";
        let app = make_app(Some(secret), None);
        let body = b"{\"action\":\"completed\",\"conclusion\":\"failure\"}";
        let sig = github_sig(secret, body);

        let req = Request::builder()
            .method(Method::POST)
            .uri("/webhooks/github")
            .header("content-type", "application/json")
            .header("x-hub-signature-256", sig)
            .body(Body::from(body.as_slice()))
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn github_no_secret_configured_skips_verification() {
        let app = make_app(None, None);
        let body = b"{}";
        let req = Request::builder()
            .method(Method::POST)
            .uri("/webhooks/github")
            .header("content-type", "application/json")
            .body(Body::from(body.as_slice()))
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    // -----------------------------------------------------------------------
    // GitLab endpoint
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn gitlab_missing_token_returns_403() {
        let app = make_app(None, Some("mytoken"));
        let body = b"{}";
        let req = Request::builder()
            .method(Method::POST)
            .uri("/webhooks/gitlab")
            .header("content-type", "application/json")
            .body(Body::from(body.as_slice()))
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn gitlab_wrong_token_returns_403() {
        let app = make_app(None, Some("mytoken"));
        let body = b"{}";
        let req = Request::builder()
            .method(Method::POST)
            .uri("/webhooks/gitlab")
            .header("content-type", "application/json")
            .header("x-gitlab-token", "wrongtoken")
            .body(Body::from(body.as_slice()))
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn gitlab_valid_token_returns_200() {
        let app = make_app(None, Some("mytoken"));
        let body = br#"{"object_kind":"pipeline","object_attributes":{"id":1,"ref":"main","sha":"abc","status":"failed"}}"#;
        let req = Request::builder()
            .method(Method::POST)
            .uri("/webhooks/gitlab")
            .header("content-type", "application/json")
            .header("x-gitlab-token", "mytoken")
            .body(Body::from(body.as_slice()))
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    // -----------------------------------------------------------------------
    // Prometheus endpoint
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn prometheus_valid_payload_returns_200_and_emits_event() {
        let (tx, mut rx) = mpsc::channel(64);
        let app = WebhookServer::new(tx).into_router();

        let body = br#"{
            "version": "4",
            "status": "firing",
            "alerts": [
                {
                    "status": "firing",
                    "labels": {"alertname": "HighErrorRate", "severity": "critical"},
                    "annotations": {"summary": "Error rate too high"}
                }
            ]
        }"#;

        let req = Request::builder()
            .method(Method::POST)
            .uri("/webhooks/prometheus")
            .header("content-type", "application/json")
            .body(Body::from(body.as_slice()))
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let event = rx.try_recv().expect("should have received one event");
        assert_eq!(
            event.source_type,
            crate::source::IntakeSourceType::Prometheus
        );
    }

    // -----------------------------------------------------------------------
    // HMAC helper unit tests
    // -----------------------------------------------------------------------

    #[test]
    fn valid_github_signature_passes() {
        let secret = b"testsecret";
        let body = b"hello world";
        let mut mac = Hmac::<Sha256>::new_from_slice(secret).unwrap();
        mac.update(body);
        let hex = format!("sha256={}", hex::encode(mac.finalize().into_bytes()));
        assert!(verify_github_signature(secret, body, &hex));
    }

    #[test]
    fn tampered_body_fails_github_signature() {
        let secret = b"testsecret";
        let body = b"hello world";
        let mut mac = Hmac::<Sha256>::new_from_slice(secret).unwrap();
        mac.update(body);
        let hex = format!("sha256={}", hex::encode(mac.finalize().into_bytes()));
        assert!(!verify_github_signature(secret, b"tampered", &hex));
    }

    #[test]
    fn missing_sha256_prefix_fails() {
        assert!(!verify_github_signature(b"secret", b"body", "abcdef"));
    }
}
