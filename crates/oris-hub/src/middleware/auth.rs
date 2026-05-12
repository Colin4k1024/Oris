use axum::body::Body;
use axum::extract::{Request, State};
use axum::middleware::Next;
use axum::response::Response;
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use std::sync::Arc;

use crate::api::AppState;
use crate::error::HubError;

pub async fn verify_api_key(
    State(state): State<Arc<AppState>>,
    req: Request,
    next: Next,
) -> Result<Response, HubError> {
    let auth_header = req.headers().get("authorization");

    match auth_header {
        Some(value) => {
            let value_str = value.to_str().unwrap_or("");
            if !value_str.starts_with("Bearer ") || value_str.len() <= 7 {
                return Err(HubError::Unauthorized("invalid bearer token format".into()));
            }
            let token = &value_str[7..];
            if !state.token_store.validate(token) {
                return Err(HubError::Unauthorized("invalid api key".into()));
            }
            Ok(next.run(req).await)
        }
        None => Err(HubError::Unauthorized(
            "missing authorization header".into(),
        )),
    }
}

pub async fn verify_ed25519_signature(
    State(state): State<Arc<AppState>>,
    req: Request,
    next: Next,
) -> Result<Response, HubError> {
    let sig_header = req
        .headers()
        .get("x-oen-signature")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    let sig_b64 = match sig_header {
        Some(s) if !s.is_empty() => s,
        _ => {
            return Err(HubError::InvalidSignature(
                "missing X-OEN-Signature header".into(),
            ))
        }
    };

    let sig_bytes = BASE64
        .decode(&sig_b64)
        .map_err(|_| HubError::InvalidSignature("invalid base64 signature".into()))?;

    let signature = Signature::from_slice(&sig_bytes)
        .map_err(|_| HubError::InvalidSignature("invalid Ed25519 signature format".into()))?;

    let (parts, body) = req.into_parts();
    let body_bytes = axum::body::to_bytes(body, 1024 * 1024)
        .await
        .map_err(|_| HubError::InvalidSignature("failed to read request body".into()))?;

    let public_key = resolve_public_key(&state, &parts, &body_bytes).await?;

    let verifying_key = VerifyingKey::from_bytes(&public_key)
        .map_err(|_| HubError::InvalidSignature("invalid public key".into()))?;

    verifying_key
        .verify(&body_bytes, &signature)
        .map_err(|_| HubError::InvalidSignature("signature verification failed".into()))?;

    let req = Request::from_parts(parts, Body::from(body_bytes));
    Ok(next.run(req).await)
}

async fn resolve_public_key(
    state: &AppState,
    parts: &axum::http::request::Parts,
    body_bytes: &[u8],
) -> Result<[u8; 32], HubError> {
    let path = parts.uri.path();

    if path == "/hub/nodes" && parts.method == axum::http::Method::POST {
        let body_json: serde_json::Value = serde_json::from_slice(body_bytes)
            .map_err(|_| HubError::InvalidSignature("invalid request body JSON".into()))?;

        let pk_b64 = body_json["public_key"]
            .as_str()
            .ok_or_else(|| HubError::InvalidSignature("missing public_key in body".into()))?;

        let pk_bytes = BASE64
            .decode(pk_b64)
            .map_err(|_| HubError::InvalidSignature("invalid base64 public_key".into()))?;

        pk_bytes
            .try_into()
            .map_err(|_| HubError::InvalidSignature("public_key must be 32 bytes".into()))
    } else {
        let node_id = extract_node_id(parts, body_bytes)?;
        let node = state.registry.get_node(&node_id).await.map_err(|_| {
            HubError::InvalidSignature("node not found for signature lookup".into())
        })?;

        let pk_bytes = BASE64.decode(&node.public_key).map_err(|_| {
            HubError::InvalidSignature("stored public_key is invalid base64".into())
        })?;

        pk_bytes
            .try_into()
            .map_err(|_| HubError::InvalidSignature("stored public_key must be 32 bytes".into()))
    }
}

fn extract_node_id(
    parts: &axum::http::request::Parts,
    body_bytes: &[u8],
) -> Result<String, HubError> {
    let path = parts.uri.path();

    // /hub/nodes/{node_id}/heartbeat or /hub/nodes/{node_id}
    if path.starts_with("/hub/nodes/") {
        let rest = &path["/hub/nodes/".len()..];
        let node_id = rest.split('/').next().unwrap_or("");
        if !node_id.is_empty() {
            return Ok(node_id.to_string());
        }
    }

    // Fallback: try to extract node_id from body JSON
    if let Ok(body_json) = serde_json::from_slice::<serde_json::Value>(body_bytes) {
        if let Some(id) = body_json["node_id"].as_str() {
            return Ok(id.to_string());
        }
        if let Some(id) = body_json["source_node_id"].as_str() {
            return Ok(id.to_string());
        }
    }

    Err(HubError::InvalidSignature(
        "cannot determine node_id for signature verification".into(),
    ))
}
