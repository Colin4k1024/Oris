//! HTTP handlers for Experience Repository.

use std::sync::Arc;

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{delete, get, post},
    Json, Router,
};
use chrono::Utc;
use oris_genestore::{Gene, GeneQuery, GeneStore};
use tokio::sync::Mutex;

use crate::api::request::{
    CreateKeyRequest, FetchQuery, RegisterPublicKeyRequest, RotateKeyRequest, ShareRequest,
};
use crate::api::response::{
    CreateKeyResponse, FetchResponse, HealthResponse, ListKeysResponse, ListPublicKeysResponse,
    NetworkAsset, PublicKeyInfo, RegisterPublicKeyResponse, RotateKeyResponse, ShareResponse,
    SyncAudit,
};
use crate::error::ExperienceRepoError;
use crate::key_service::{KeyId, KeyStore};
use crate::oen::OenVerifier;
use crate::server::middleware::rate_limit::{RateLimitConfig, RateLimiterRegistry};
use crate::server::ServerConfig;

/// Extract client identifier from headers for rate limiting.
/// Uses X-Forwarded-For or X-Real-IP headers, falling back to "default".
fn extract_client_id(headers: &axum::http::HeaderMap) -> String {
    headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(',').next())
        .map(|v| v.trim().to_string())
        .or_else(|| {
            headers
                .get("x-real-ip")
                .and_then(|v| v.to_str().ok())
                .map(|v| v.to_string())
        })
        .unwrap_or_else(|| "default".to_string())
}

/// Check rate limit for a request, returning error if exceeded.
fn check_rate_limit(
    rate_limiter: &RateLimiterRegistry,
    method: axum::http::Method,
    path: &str,
    client_id: &str,
) -> Result<(), ExperienceRepoError> {
    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    rate_limiter
        .check(&method, path, client_id, now_secs)
        .map_err(ExperienceRepoError::RateLimitExceeded)
}

/// Application state shared across handlers.
#[derive(Clone)]
pub struct AppState {
    pub store: Arc<Mutex<dyn GeneStore>>,
    pub key_store: Arc<Mutex<KeyStore>>,
    pub oen_verifier: OenVerifier,
    pub rate_limiter: RateLimiterRegistry,
}

/// Create the router with all routes.
pub fn create_routes(config: ServerConfig) -> Router {
    let store: Arc<Mutex<dyn GeneStore>> = Arc::new(Mutex::new(
        oris_genestore::SqliteGeneStore::open(&config.store_path)
            .expect("failed to open gene store"),
    ));

    // Open or create the key store
    let key_store_path = config.key_store_path.clone();
    let key_store = KeyStore::open(&key_store_path).expect("failed to open key store");

    let state = AppState {
        store,
        key_store: Arc::new(Mutex::new(key_store)),
        oen_verifier: OenVerifier::new(),
        rate_limiter: RateLimiterRegistry::new(&RateLimitConfig::default()),
    };

    Router::new()
        // Experience endpoints
        .route("/experience", get(fetch_experiences))
        .route("/experience", post(share_experience))
        // Key management endpoints
        .route("/keys", get(list_keys))
        .route("/keys", post(create_key))
        .route("/keys/:key_id", delete(revoke_key))
        .route("/keys/:key_id/rotate", post(rotate_key))
        // Public key management endpoints (PKI)
        .route("/public-keys", get(list_public_keys))
        .route("/public-keys", post(register_public_key))
        .route("/public-keys/:sender_id", delete(revoke_public_key))
        // Health
        .route("/health", get(health))
        .with_state(state)
}

// ============================================================================
// Experience API Handlers
// ============================================================================

/// Handler for GET /experience - fetch matching experiences.
async fn fetch_experiences(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Query(query): Query<FetchQuery>,
) -> Result<Json<FetchResponse>, ExperienceRepoError> {
    // Check rate limit for GET /experience
    let client_id = extract_client_id(&headers);
    check_rate_limit(
        &state.rate_limiter,
        axum::http::Method::GET,
        "/experience",
        &client_id,
    )?;

    let signals = query.signals();
    let limit = query.limit;
    let min_confidence = query.min_confidence;

    // Build gene query
    let gene_query = GeneQuery {
        min_confidence,
        limit,
        required_tags: vec![],
        problem_description: signals.join(","),
    };

    // Search genes
    let store = state.store.lock().await;

    let matches = store.search_genes(&gene_query).await.map_err(|e| {
        ExperienceRepoError::GeneStoreError(anyhow::anyhow!("search failed: {}", e))
    })?;

    drop(store);

    let scanned_count = matches.len();
    let assets: Vec<NetworkAsset> = matches
        .into_iter()
        .map(|m| {
            let gene = m.gene;
            NetworkAsset::Gene {
                id: gene.id.to_string(),
                signals: gene.tags,
                strategy: gene.template.lines().map(|s| s.to_string()).collect(),
                validation: gene.validation_steps,
                confidence: gene.confidence,
                quality_score: gene.quality_score,
                use_count: gene.use_count,
                success_count: gene.success_count,
                created_at: gene.created_at.to_rfc3339(),
                contributor_id: None, // TODO: enrich from metadata
            }
        })
        .collect();

    Ok(Json(FetchResponse {
        assets,
        next_cursor: None,
        sync_audit: SyncAudit {
            scanned_count,
            applied_count: scanned_count,
            skipped_count: 0,
            failed_count: 0,
        },
    }))
}

/// Handler for POST /experience - share an experience.
async fn share_experience(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(request): Json<ShareRequest>,
) -> Result<Json<ShareResponse>, ExperienceRepoError> {
    // Check rate limit for POST /experience
    let client_id = extract_client_id(&headers);
    check_rate_limit(
        &state.rate_limiter,
        axum::http::Method::POST,
        "/experience",
        &client_id,
    )?;

    // Extract API key from header
    let api_key = headers
        .get("X-Api-Key")
        .and_then(|v| v.to_str().ok())
        .ok_or(ExperienceRepoError::ApiKeyMissing)?;

    // Verify API key
    let key_info = state.key_store.lock().await.verify_key(api_key)?;

    // Lookup public key for sender and verify Ed25519 signature
    let public_key = state
        .key_store
        .lock()
        .await
        .get_public_key(&request.envelope.sender_id)?
        .ok_or(ExperienceRepoError::PublicKeyNotFound)?;

    // Verify Ed25519 signature (verifies message type, timestamp, and signature)
    state
        .oen_verifier
        .verify_envelope(
            &request.envelope,
            &key_info.agent_id,
            &public_key.public_key_hex,
        )
        .await?;

    // Validate sender_id matches API key's agent_id
    if request.envelope.sender_id != key_info.agent_id {
        return Err(ExperienceRepoError::SenderMismatch);
    }

    // Extract gene from envelope payload
    let payload = request.envelope.payload;
    let gene: Gene =
        serde_json::from_value(payload).map_err(|_| ExperienceRepoError::InvalidEnvelope)?;

    // Store the gene
    let store = state.store.lock().await;
    store
        .upsert_gene(&gene)
        .await
        .map_err(|e| ExperienceRepoError::GeneStoreError(anyhow::anyhow!("store failed: {}", e)))?;

    let now = Utc::now().to_rfc3339();

    Ok(Json(ShareResponse {
        gene_id: gene.id.to_string(),
        status: "published".to_string(),
        published_at: now,
    }))
}

// ============================================================================
// Key Management Handlers
// ============================================================================

/// Handler for GET /keys - list all API keys.
async fn list_keys(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
) -> Result<Json<ListKeysResponse>, ExperienceRepoError> {
    // Check rate limit for key management endpoints
    let client_id = extract_client_id(&headers);
    check_rate_limit(
        &state.rate_limiter,
        axum::http::Method::GET,
        "/keys",
        &client_id,
    )?;

    let keys = state.key_store.lock().await.list_keys()?;
    Ok(Json(ListKeysResponse { keys }))
}

/// Handler for POST /keys - create a new API key.
async fn create_key(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(request): Json<CreateKeyRequest>,
) -> Result<Json<CreateKeyResponse>, ExperienceRepoError> {
    // Check rate limit for key management endpoints
    let client_id = extract_client_id(&headers);
    check_rate_limit(
        &state.rate_limiter,
        axum::http::Method::POST,
        "/keys",
        &client_id,
    )?;

    let (raw_key, key_info) = state.key_store.lock().await.create_key(
        &request.agent_id,
        request.description,
        request.ttl_days,
    )?;

    Ok(Json(CreateKeyResponse {
        key_id: key_info.key_id.0,
        api_key: raw_key,
        agent_id: key_info.agent_id,
        created_at: key_info.created_at.to_rfc3339(),
        expires_at: key_info.expires_at.map(|dt| dt.to_rfc3339()),
    }))
}

/// Handler for DELETE /keys/:key_id - revoke an API key.
async fn revoke_key(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path(key_id): Path<String>,
) -> Result<StatusCode, ExperienceRepoError> {
    // Check rate limit for key management endpoints
    let client_id = extract_client_id(&headers);
    check_rate_limit(
        &state.rate_limiter,
        axum::http::Method::DELETE,
        "/keys",
        &client_id,
    )?;

    let key_id = KeyId(key_id);
    state.key_store.lock().await.revoke_key(&key_id)?;
    Ok(StatusCode::NO_CONTENT)
}

/// Handler for POST /keys/:key_id/rotate - rotate an API key.
async fn rotate_key(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path(key_id): Path<String>,
    Json(request): Json<RotateKeyRequest>,
) -> Result<Json<RotateKeyResponse>, ExperienceRepoError> {
    // Check rate limit for key management endpoints
    let client_id = extract_client_id(&headers);
    check_rate_limit(
        &state.rate_limiter,
        axum::http::Method::POST,
        "/keys",
        &client_id,
    )?;

    let key_id = KeyId(key_id);
    let (raw_key, key_info) = state
        .key_store
        .lock()
        .await
        .rotate_key(&key_id, request.ttl_days)?;

    Ok(Json(RotateKeyResponse {
        key_id: key_info.key_id.0,
        api_key: raw_key,
        rotated_at: Utc::now().to_rfc3339(),
    }))
}

// ============================================================================
// Public Key Management Handlers (PKI)
// ============================================================================

/// Handler for GET /public-keys - list all public keys (requires API key).
async fn list_public_keys(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
) -> Result<Json<ListPublicKeysResponse>, ExperienceRepoError> {
    // Check rate limit for key management endpoints
    let client_id = extract_client_id(&headers);
    check_rate_limit(
        &state.rate_limiter,
        axum::http::Method::GET,
        "/public-keys",
        &client_id,
    )?;

    // Require API key authentication
    let _api_key = headers
        .get("X-Api-Key")
        .and_then(|v| v.to_str().ok())
        .ok_or(ExperienceRepoError::ApiKeyMissing)?;

    let keys = state.key_store.lock().await.list_public_keys()?;
    let public_keys: Vec<PublicKeyInfo> = keys.iter().map(PublicKeyInfo::from).collect();
    Ok(Json(ListPublicKeysResponse { keys: public_keys }))
}

/// Handler for POST /public-keys - register a public key (requires API key).
async fn register_public_key(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(request): Json<RegisterPublicKeyRequest>,
) -> Result<Json<RegisterPublicKeyResponse>, ExperienceRepoError> {
    // Check rate limit for key management endpoints
    let client_id = extract_client_id(&headers);
    check_rate_limit(
        &state.rate_limiter,
        axum::http::Method::POST,
        "/public-keys",
        &client_id,
    )?;

    // Require API key authentication
    let api_key = headers
        .get("X-Api-Key")
        .and_then(|v| v.to_str().ok())
        .ok_or(ExperienceRepoError::ApiKeyMissing)?;

    // Verify the API key and get agent_id
    let key_info = state.key_store.lock().await.verify_key(api_key)?;

    // Only allow registering a public key for the agent_id associated with the API key
    if request.sender_id != key_info.agent_id {
        return Err(ExperienceRepoError::SenderMismatch);
    }

    let public_key = state
        .key_store
        .lock()
        .await
        .register_public_key(&request.sender_id, &request.public_key_hex)?;

    Ok(Json(RegisterPublicKeyResponse {
        sender_id: public_key.sender_id,
        version: public_key.version,
        created_at: public_key.created_at.to_rfc3339(),
    }))
}

/// Handler for DELETE /public-keys/:sender_id - revoke a public key (requires API key).
async fn revoke_public_key(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path(sender_id): Path<String>,
) -> Result<StatusCode, ExperienceRepoError> {
    // Check rate limit for key management endpoints
    let client_id = extract_client_id(&headers);
    check_rate_limit(
        &state.rate_limiter,
        axum::http::Method::DELETE,
        "/public-keys",
        &client_id,
    )?;

    // Require API key authentication
    let api_key = headers
        .get("X-Api-Key")
        .and_then(|v| v.to_str().ok())
        .ok_or(ExperienceRepoError::ApiKeyMissing)?;

    // Verify the API key and get agent_id
    let key_info = state.key_store.lock().await.verify_key(api_key)?;

    // Only allow revoking a public key for the agent_id associated with the API key
    if sender_id != key_info.agent_id {
        return Err(ExperienceRepoError::SenderMismatch);
    }

    state.key_store.lock().await.revoke_public_key(&sender_id)?;
    Ok(StatusCode::NO_CONTENT)
}

// ============================================================================
// Health Check
// ============================================================================

/// Handler for GET /health - health check (no auth required).
async fn health() -> Json<HealthResponse> {
    Json(HealthResponse::ok())
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::request::FetchQuery;
    use crate::key_service::KeyStore;
    use axum::http::HeaderMap;

    fn create_test_state() -> AppState {
        let store = oris_genestore::SqliteGeneStore::open(":memory:").unwrap();
        let key_store = KeyStore::memory().unwrap();

        AppState {
            store: Arc::new(Mutex::new(store)),
            key_store: Arc::new(Mutex::new(key_store)),
            oen_verifier: OenVerifier::new(),
            rate_limiter: RateLimiterRegistry::new(&RateLimitConfig::default()),
        }
    }

    fn create_test_headers() -> HeaderMap {
        HeaderMap::new()
    }

    #[tokio::test]
    async fn test_fetch_experiences_empty() {
        let state = create_test_state();

        let query = FetchQuery {
            q: Some("timeout".to_string()),
            min_confidence: 0.5,
            limit: 10,
            cursor: None,
        };

        let result = fetch_experiences(State(state), create_test_headers(), Query(query)).await;
        assert!(result.is_ok());
        let response = result.unwrap().0;
        assert!(response.assets.is_empty());
    }

    #[tokio::test]
    async fn test_health() {
        let response = health().await;
        assert_eq!(response.0.status, "ok");
    }

    #[tokio::test]
    async fn test_create_and_list_key() {
        let state = create_test_state();
        let headers = create_test_headers();

        // Create a key
        let create_request = CreateKeyRequest {
            agent_id: "agent-123".to_string(),
            ttl_days: Some(30),
            description: Some("test key".to_string()),
        };

        let create_response =
            create_key(State(state.clone()), headers.clone(), Json(create_request))
                .await
                .unwrap();
        assert_eq!(create_response.agent_id, "agent-123");
        assert!(create_response.api_key.starts_with("sk_live_"));

        // List keys
        let list_response = list_keys(State(state), headers).await.unwrap();
        assert_eq!(list_response.keys.len(), 1);
        assert_eq!(list_response.keys[0].agent_id, "agent-123");
    }

    #[tokio::test]
    async fn test_revoke_key() {
        let state = create_test_state();
        let headers = create_test_headers();

        // Create a key
        let create_request = CreateKeyRequest {
            agent_id: "agent-123".to_string(),
            ttl_days: None,
            description: None,
        };

        let create_response =
            create_key(State(state.clone()), headers.clone(), Json(create_request))
                .await
                .unwrap();

        // Revoke the key
        let revoke_result = revoke_key(
            State(state.clone()),
            headers,
            Path(create_response.key_id.clone()),
        )
        .await;
        assert!(revoke_result.is_ok());

        // Verify the key is revoked
        let verify_result = state
            .key_store
            .lock()
            .await
            .verify_key(&create_response.api_key);
        assert!(verify_result.is_err());
    }
}
