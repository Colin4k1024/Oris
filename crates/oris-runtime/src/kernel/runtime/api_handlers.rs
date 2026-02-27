//! Axum handlers for Phase 2 execution server.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::{header::AUTHORIZATION, HeaderMap};
use axum::middleware::{from_fn, from_fn_with_state, Next};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{Duration, Utc};
use serde_json::Value;
use sha2::{Digest, Sha256};
use tokio::sync::RwLock;

use crate::graph::{Command, CompiledGraph, MessagesState, RunnableConfig, StateOrCommand};
use crate::schemas::messages::Message;

use super::api_errors::ApiError;
#[cfg(feature = "sqlite-persistence")]
use super::api_idempotency::{IdempotencyRecord, SqliteIdempotencyStore};
use super::api_models::{
    ApiEnvelope, ApiMeta, CancelJobRequest, CancelJobResponse, CheckpointInspectResponse,
    InterruptDetailResponse, InterruptListItem, InterruptListResponse, JobDetailResponse,
    JobHistoryItem, JobHistoryResponse, JobListItem, JobStateResponse, JobTimelineItem,
    JobTimelineResponse, ListJobsResponse, RejectInterruptRequest, ReplayJobRequest,
    ResumeInterruptRequest, ResumeJobRequest, RunJobRequest, RunJobResponse,
    TimelineExportResponse, WorkerAckRequest, WorkerAckResponse, WorkerExtendLeaseRequest,
    WorkerHeartbeatRequest, WorkerLeaseResponse, WorkerPollRequest, WorkerPollResponse,
    WorkerReportStepRequest,
};
use super::lease::{LeaseConfig, LeaseManager, RepositoryLeaseManager};
use super::models::AttemptExecutionStatus;
use super::scheduler::{SchedulerDecision, SkeletonScheduler};
#[cfg(feature = "sqlite-persistence")]
use super::sqlite_runtime_repository::{SqliteRuntimeRepository, StepReportWriteResult};

#[derive(Clone, Debug, Default)]
pub struct ExecutionApiAuthConfig {
    pub bearer_token: Option<String>,
    pub api_key_hash: Option<String>,
    pub keyed_api_keys: HashMap<String, StaticApiKeyConfig>,
}

#[derive(Clone, Debug)]
pub struct StaticApiKeyConfig {
    pub secret_hash: String,
    pub active: bool,
}

impl ExecutionApiAuthConfig {
    fn normalize_secret(secret: Option<String>) -> Option<String> {
        secret.and_then(|v| {
            let trimmed = v.trim().to_string();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed)
            }
        })
    }

    fn normalize_key_id(key_id: Option<String>) -> Option<String> {
        key_id.and_then(|v| {
            let trimmed = v.trim().to_string();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed)
            }
        })
    }

    fn secret_hash(secret: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(secret.as_bytes());
        format!("{:x}", hasher.finalize())
    }

    fn normalize_hashed_secret(secret: Option<String>) -> Option<String> {
        Self::normalize_secret(secret).map(|value| Self::secret_hash(value.as_str()))
    }

    fn from_optional(bearer_token: Option<String>, api_key: Option<String>) -> Self {
        Self {
            bearer_token: Self::normalize_secret(bearer_token),
            api_key_hash: Self::normalize_hashed_secret(api_key),
            keyed_api_keys: HashMap::new(),
        }
    }

    fn set_keyed_api_key(&mut self, key_id: String, secret: String, active: bool) {
        if let (Some(key_id), Some(secret)) = (
            Self::normalize_key_id(Some(key_id)),
            Self::normalize_secret(Some(secret)),
        ) {
            self.keyed_api_keys.insert(
                key_id,
                StaticApiKeyConfig {
                    secret_hash: Self::secret_hash(secret.as_str()),
                    active,
                },
            );
        }
    }

    fn is_enabled(&self) -> bool {
        self.bearer_token.is_some()
            || self.api_key_hash.is_some()
            || !self.keyed_api_keys.is_empty()
    }
}

#[derive(Clone)]
pub struct ExecutionApiState {
    pub compiled: Arc<CompiledGraph<MessagesState>>,
    pub cancelled_threads: Arc<RwLock<HashSet<String>>>,
    pub auth: ExecutionApiAuthConfig,
    #[cfg(feature = "sqlite-persistence")]
    pub idempotency_store: Option<SqliteIdempotencyStore>,
    #[cfg(feature = "sqlite-persistence")]
    pub runtime_repo: Option<SqliteRuntimeRepository>,
    pub worker_poll_limit: usize,
    pub max_active_leases_per_worker: usize,
}

impl ExecutionApiState {
    pub fn new(compiled: Arc<CompiledGraph<MessagesState>>) -> Self {
        Self {
            compiled,
            cancelled_threads: Arc::new(RwLock::new(HashSet::new())),
            auth: ExecutionApiAuthConfig::default(),
            #[cfg(feature = "sqlite-persistence")]
            idempotency_store: None,
            #[cfg(feature = "sqlite-persistence")]
            runtime_repo: None,
            worker_poll_limit: 1,
            max_active_leases_per_worker: 8,
        }
    }

    #[cfg(feature = "sqlite-persistence")]
    pub fn with_sqlite_idempotency(
        compiled: Arc<CompiledGraph<MessagesState>>,
        db_path: &str,
    ) -> Self {
        let mut state = Self::new(compiled);
        if let Ok(store) = SqliteIdempotencyStore::new(db_path) {
            state.idempotency_store = Some(store);
        }
        if let Ok(repo) = SqliteRuntimeRepository::new(db_path) {
            state.runtime_repo = Some(repo);
        }
        state
    }

    pub fn with_static_auth(
        mut self,
        bearer_token: Option<String>,
        api_key: Option<String>,
    ) -> Self {
        self.auth = ExecutionApiAuthConfig::from_optional(bearer_token, api_key);
        self
    }

    pub fn with_static_bearer_token(mut self, token: impl Into<String>) -> Self {
        self.auth.bearer_token = ExecutionApiAuthConfig::normalize_secret(Some(token.into()));
        self
    }

    pub fn with_static_api_key(mut self, key: impl Into<String>) -> Self {
        self.auth.api_key_hash = ExecutionApiAuthConfig::normalize_hashed_secret(Some(key.into()));
        self
    }

    pub fn with_static_api_key_record(
        mut self,
        key_id: impl Into<String>,
        secret: impl Into<String>,
        active: bool,
    ) -> Self {
        self.auth
            .set_keyed_api_key(key_id.into(), secret.into(), active);
        self
    }
}

pub fn build_router(state: ExecutionApiState) -> Router {
    Router::new()
        .route("/v1/jobs", get(list_jobs).post(run_job))
        .route("/v1/jobs/run", post(run_job))
        .route("/v1/jobs/:thread_id", get(inspect_job))
        .route("/v1/jobs/:thread_id/detail", get(job_detail))
        .route("/v1/jobs/:thread_id/timeline/export", get(export_timeline))
        .route("/v1/jobs/:thread_id/history", get(job_history))
        .route("/v1/jobs/:thread_id/timeline", get(job_timeline))
        .route(
            "/v1/jobs/:thread_id/checkpoints/:checkpoint_id",
            get(inspect_checkpoint),
        )
        .route("/v1/jobs/:thread_id/resume", post(resume_job))
        .route("/v1/jobs/:thread_id/replay", post(replay_job))
        .route("/v1/jobs/:thread_id/cancel", post(cancel_job))
        .route("/v1/workers/poll", post(worker_poll))
        .route("/v1/workers/:worker_id/heartbeat", post(worker_heartbeat))
        .route(
            "/v1/workers/:worker_id/extend-lease",
            post(worker_extend_lease),
        )
        .route(
            "/v1/workers/:worker_id/report-step",
            post(worker_report_step),
        )
        .route("/v1/workers/:worker_id/ack", post(worker_ack))
        .route("/v1/interrupts", get(list_interrupts))
        .route("/v1/interrupts/:interrupt_id", get(get_interrupt))
        .route(
            "/v1/interrupts/:interrupt_id/resume",
            post(resume_interrupt),
        )
        .route(
            "/v1/interrupts/:interrupt_id/reject",
            post(reject_interrupt),
        )
        .layer(from_fn_with_state(state.clone(), auth_middleware))
        .layer(from_fn(request_log_middleware))
        .with_state(state)
}

fn request_id(headers: &HeaderMap) -> String {
    headers
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string())
}

fn payload_hash(thread_id: &str, input: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(thread_id.as_bytes());
    hasher.update(b"|");
    hasher.update(input.as_bytes());
    format!("{:x}", hasher.finalize())
}

fn json_hash(value: &Value) -> Result<String, ApiError> {
    let json = serde_json::to_vec(value)
        .map_err(|e| ApiError::internal(format!("serialize json: {}", e)))?;
    let mut hasher = Sha256::new();
    hasher.update(&json);
    Ok(format!("{:x}", hasher.finalize()))
}

fn bearer_token_from_headers(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(str::trim)
        .filter(|v| !v.is_empty())
}

fn api_key_from_headers(headers: &HeaderMap) -> Option<&str> {
    headers
        .get("x-api-key")
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|v| !v.is_empty())
}

fn api_key_id_from_headers(headers: &HeaderMap) -> Option<&str> {
    headers
        .get("x-api-key-id")
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|v| !v.is_empty())
}

fn is_authorized(headers: &HeaderMap, auth: &ExecutionApiAuthConfig) -> bool {
    let bearer_ok = auth
        .bearer_token
        .as_deref()
        .zip(bearer_token_from_headers(headers))
        .map(|(expected, actual)| expected == actual)
        .unwrap_or(false);
    let api_key_ok = auth
        .api_key_hash
        .as_deref()
        .zip(api_key_from_headers(headers))
        .map(|(expected_hash, actual)| expected_hash == ExecutionApiAuthConfig::secret_hash(actual))
        .unwrap_or(false);
    let keyed_api_key_ok = api_key_id_from_headers(headers)
        .zip(api_key_from_headers(headers))
        .and_then(|(key_id, secret)| {
            auth.keyed_api_keys.get(key_id).map(|config| {
                config.active && config.secret_hash == ExecutionApiAuthConfig::secret_hash(secret)
            })
        })
        .unwrap_or(false);
    bearer_ok || api_key_ok || keyed_api_key_ok
}

async fn auth_middleware(
    State(state): State<ExecutionApiState>,
    headers: HeaderMap,
    request: axum::extract::Request,
    next: Next,
) -> axum::response::Response {
    if !state.auth.is_enabled() || is_authorized(&headers, &state.auth) {
        return next.run(request).await;
    }

    let mut methods = Vec::new();
    if state.auth.bearer_token.is_some() {
        methods.push("bearer");
    }
    if state.auth.api_key_hash.is_some() {
        methods.push("x-api-key");
    }
    if !state.auth.keyed_api_keys.is_empty() {
        methods.push("x-api-key-id+x-api-key");
    }

    let rid = request_id(&headers);
    ApiError::unauthorized("missing or invalid credentials")
        .with_request_id(rid)
        .with_details(serde_json::json!({ "supported_auth": methods }))
        .into_response()
}

async fn request_log_middleware(
    headers: HeaderMap,
    request: axum::extract::Request,
    next: Next,
) -> axum::response::Response {
    let rid = request_id(&headers);
    log::info!(
        "execution_api_request request_id={} method={} path={}",
        rid,
        request.method(),
        request.uri().path()
    );
    next.run(request).await
}

fn validate_thread_id(thread_id: &str) -> Result<(), ApiError> {
    if thread_id.trim().is_empty() {
        return Err(ApiError::bad_request("thread_id must not be empty"));
    }
    Ok(())
}

fn validate_worker_id(worker_id: &str) -> Result<(), ApiError> {
    if worker_id.trim().is_empty() {
        return Err(ApiError::bad_request("worker_id must not be empty"));
    }
    Ok(())
}

async fn ensure_not_cancelled(state: &ExecutionApiState, thread_id: &str) -> Result<(), ApiError> {
    if state.cancelled_threads.read().await.contains(thread_id) {
        return Err(ApiError::conflict(format!(
            "thread '{}' is cancelled",
            thread_id
        )));
    }
    Ok(())
}

#[cfg(feature = "sqlite-persistence")]
fn runtime_repo<'a>(
    state: &'a ExecutionApiState,
    rid: &str,
) -> Result<&'a SqliteRuntimeRepository, ApiError> {
    state.runtime_repo.as_ref().ok_or_else(|| {
        ApiError::internal("runtime repository is not configured").with_request_id(rid.to_string())
    })
}

pub async fn run_job(
    State(state): State<ExecutionApiState>,
    headers: HeaderMap,
    Json(req): Json<RunJobRequest>,
) -> Result<Json<ApiEnvelope<RunJobResponse>>, ApiError> {
    let rid = request_id(&headers);
    validate_thread_id(&req.thread_id).map_err(|e| e.with_request_id(rid.clone()))?;
    ensure_not_cancelled(&state, &req.thread_id)
        .await
        .map_err(|e| e.with_request_id(rid.clone()))?;

    let input = req.input.unwrap_or_else(|| "API run".to_string());
    let request_payload_hash = payload_hash(&req.thread_id, &input);
    log::info!(
        "execution_run request_id={} thread_id={} checkpoint_id=none",
        rid,
        req.thread_id
    );

    #[cfg(feature = "sqlite-persistence")]
    if let (Some(key), Some(store)) = (
        req.idempotency_key.clone(),
        state.idempotency_store.as_ref(),
    ) {
        if key.trim().is_empty() {
            return Err(ApiError::bad_request("idempotency_key must not be empty")
                .with_request_id(rid.clone()));
        }
        if let Some(existing) = store
            .get(&key)
            .map_err(|e| ApiError::internal(e).with_request_id(rid.clone()))?
        {
            if existing.operation == "run"
                && existing.thread_id == req.thread_id
                && existing.payload_hash == request_payload_hash
            {
                let mut response: RunJobResponse = serde_json::from_str(&existing.response_json)
                    .map_err(|e| {
                        ApiError::internal(format!("decode idempotent response failed: {}", e))
                            .with_request_id(rid.clone())
                    })?;
                response.idempotent_replay = true;
                return Ok(Json(ApiEnvelope {
                    meta: ApiMeta::ok(),
                    request_id: rid,
                    data: response,
                }));
            }
            return Err(ApiError::conflict(
                "idempotency_key already exists with different request payload",
            )
            .with_request_id(rid.clone())
            .with_details(serde_json::json!({
                "idempotency_key": key,
                "operation": existing.operation
            })));
        }
    }

    let initial = MessagesState::with_messages(vec![Message::new_human_message(input)]);
    let config = RunnableConfig::with_thread_id(&req.thread_id);
    let result = state
        .compiled
        .invoke_with_config_interrupt(StateOrCommand::State(initial), &config)
        .await
        .map_err(|e| {
            ApiError::internal(format!("run failed: {}", e)).with_request_id(rid.clone())
        })?;

    let interrupts = result
        .interrupt
        .unwrap_or_default()
        .into_iter()
        .map(|i| i.value)
        .collect::<Vec<_>>();
    let status = if interrupts.is_empty() {
        "completed".to_string()
    } else {
        "interrupted".to_string()
    };

    let response = RunJobResponse {
        thread_id: req.thread_id.clone(),
        status: status.clone(),
        interrupts: interrupts.clone(),
        idempotency_key: req.idempotency_key.clone(),
        idempotent_replay: false,
    };

    #[cfg(feature = "sqlite-persistence")]
    if let Some(repo) = state.runtime_repo.as_ref() {
        let _ = repo.upsert_job(&req.thread_id, &status);
        let _ = repo.enqueue_attempt(
            &format!("attempt-{}-{}", req.thread_id, uuid::Uuid::new_v4()),
            &req.thread_id,
        );
        if !interrupts.is_empty() {
            let attempt_id = format!("attempt-{}-main", req.thread_id);
            for (i, iv) in interrupts.iter().enumerate() {
                let interrupt_id = format!("int-{}-{}", req.thread_id, i);
                let value_json = serde_json::to_string(iv).unwrap_or_default();
                let _ = repo.insert_interrupt(
                    &interrupt_id,
                    &req.thread_id,
                    &req.thread_id,
                    &attempt_id,
                    &value_json,
                );
            }
        }
    }

    #[cfg(feature = "sqlite-persistence")]
    if let (Some(key), Some(store)) = (
        req.idempotency_key.clone(),
        state.idempotency_store.as_ref(),
    ) {
        let record = IdempotencyRecord {
            operation: "run".to_string(),
            thread_id: req.thread_id.clone(),
            payload_hash: request_payload_hash,
            response_json: serde_json::to_string(&response).map_err(|e| {
                ApiError::internal(format!("encode idempotent response failed: {}", e))
                    .with_request_id(rid.clone())
            })?,
        };
        store
            .put(&key, &record)
            .map_err(|e| ApiError::internal(e).with_request_id(rid.clone()))?;
    }

    Ok(Json(ApiEnvelope {
        meta: ApiMeta::ok(),
        request_id: rid,
        data: response,
    }))
}

pub async fn inspect_job(
    State(state): State<ExecutionApiState>,
    Path(thread_id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<ApiEnvelope<JobStateResponse>>, ApiError> {
    let rid = request_id(&headers);
    validate_thread_id(&thread_id).map_err(|e| e.with_request_id(rid.clone()))?;
    let config = RunnableConfig::with_thread_id(&thread_id);
    log::info!(
        "execution_inspect request_id={} thread_id={} checkpoint_id=none",
        rid,
        thread_id
    );
    let snapshot = state.compiled.get_state(&config).await.map_err(|e| {
        if e.to_string().contains("No state found") {
            ApiError::not_found(e.to_string()).with_request_id(rid.clone())
        } else {
            ApiError::internal(format!("inspect failed: {}", e)).with_request_id(rid.clone())
        }
    })?;

    let checkpoint_id = snapshot.checkpoint_id().cloned();
    let created_at = snapshot.created_at.to_rfc3339();
    let values = serde_json::to_value(&snapshot.values).map_err(|e| {
        ApiError::internal(format!("serialize state failed: {}", e)).with_request_id(rid.clone())
    })?;

    Ok(Json(ApiEnvelope {
        meta: ApiMeta::ok(),
        request_id: rid,
        data: JobStateResponse {
            thread_id,
            checkpoint_id,
            created_at,
            values,
        },
    }))
}

pub async fn job_history(
    State(state): State<ExecutionApiState>,
    Path(thread_id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<ApiEnvelope<JobHistoryResponse>>, ApiError> {
    let rid = request_id(&headers);
    validate_thread_id(&thread_id).map_err(|e| e.with_request_id(rid.clone()))?;
    let config = RunnableConfig::with_thread_id(&thread_id);
    log::info!(
        "execution_history request_id={} thread_id={} checkpoint_id=none",
        rid,
        thread_id
    );
    let history = state
        .compiled
        .get_state_history(&config)
        .await
        .map_err(|e| {
            ApiError::internal(format!("history failed: {}", e)).with_request_id(rid.clone())
        })?;

    let items = history
        .iter()
        .map(|s| JobHistoryItem {
            checkpoint_id: s.checkpoint_id().cloned(),
            created_at: s.created_at.to_rfc3339(),
        })
        .collect();

    Ok(Json(ApiEnvelope {
        meta: ApiMeta::ok(),
        request_id: rid,
        data: JobHistoryResponse {
            thread_id,
            history: items,
        },
    }))
}

pub async fn job_timeline(
    State(state): State<ExecutionApiState>,
    Path(thread_id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<ApiEnvelope<JobTimelineResponse>>, ApiError> {
    let rid = request_id(&headers);
    validate_thread_id(&thread_id).map_err(|e| e.with_request_id(rid.clone()))?;
    let config = RunnableConfig::with_thread_id(&thread_id);
    log::info!(
        "execution_timeline request_id={} thread_id={} checkpoint_id=none",
        rid,
        thread_id
    );
    let history = state
        .compiled
        .get_state_history(&config)
        .await
        .map_err(|e| {
            ApiError::internal(format!("timeline failed: {}", e)).with_request_id(rid.clone())
        })?;
    if history.is_empty() {
        return Err(
            ApiError::not_found(format!("No timeline found for thread: {}", thread_id))
                .with_request_id(rid.clone()),
        );
    }
    let timeline = history
        .iter()
        .enumerate()
        .map(|(i, s)| JobTimelineItem {
            seq: (i + 1) as u64,
            event_type: "checkpoint_saved".to_string(),
            checkpoint_id: s.checkpoint_id().cloned(),
            created_at: s.created_at.to_rfc3339(),
        })
        .collect();

    Ok(Json(ApiEnvelope {
        meta: ApiMeta::ok(),
        request_id: rid,
        data: JobTimelineResponse {
            thread_id,
            timeline,
        },
    }))
}

pub async fn inspect_checkpoint(
    State(state): State<ExecutionApiState>,
    Path((thread_id, checkpoint_id)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<Json<ApiEnvelope<CheckpointInspectResponse>>, ApiError> {
    let rid = request_id(&headers);
    validate_thread_id(&thread_id).map_err(|e| e.with_request_id(rid.clone()))?;
    if checkpoint_id.trim().is_empty() {
        return Err(
            ApiError::bad_request("checkpoint_id must not be empty").with_request_id(rid.clone())
        );
    }
    log::info!(
        "execution_checkpoint_inspect request_id={} thread_id={} checkpoint_id={}",
        rid,
        thread_id,
        checkpoint_id
    );
    let config = RunnableConfig::with_checkpoint(&thread_id, &checkpoint_id);
    let snapshot = state.compiled.get_state(&config).await.map_err(|e| {
        if e.to_string().contains("No state found")
            || e.to_string().contains("Checkpoint not found")
        {
            ApiError::not_found(e.to_string()).with_request_id(rid.clone())
        } else {
            ApiError::internal(format!("inspect checkpoint failed: {}", e))
                .with_request_id(rid.clone())
        }
    })?;
    let created_at = snapshot.created_at.to_rfc3339();
    let values = serde_json::to_value(&snapshot.values).map_err(|e| {
        ApiError::internal(format!("serialize state failed: {}", e)).with_request_id(rid.clone())
    })?;

    Ok(Json(ApiEnvelope {
        meta: ApiMeta::ok(),
        request_id: rid,
        data: CheckpointInspectResponse {
            thread_id,
            checkpoint_id,
            created_at,
            values,
        },
    }))
}

pub async fn resume_job(
    State(state): State<ExecutionApiState>,
    Path(thread_id): Path<String>,
    headers: HeaderMap,
    Json(req): Json<ResumeJobRequest>,
) -> Result<Json<ApiEnvelope<RunJobResponse>>, ApiError> {
    let rid = request_id(&headers);
    validate_thread_id(&thread_id).map_err(|e| e.with_request_id(rid.clone()))?;
    ensure_not_cancelled(&state, &thread_id)
        .await
        .map_err(|e| e.with_request_id(rid.clone()))?;

    let config = if let Some(cp) = req.checkpoint_id.as_ref() {
        RunnableConfig::with_checkpoint(&thread_id, cp)
    } else {
        RunnableConfig::with_thread_id(&thread_id)
    };
    log::info!(
        "execution_resume request_id={} thread_id={} checkpoint_id={}",
        rid,
        thread_id,
        req.checkpoint_id
            .clone()
            .unwrap_or_else(|| "none".to_string())
    );

    let result = state
        .compiled
        .invoke_with_config_interrupt(StateOrCommand::Command(Command::resume(req.value)), &config)
        .await
        .map_err(|e| {
            ApiError::internal(format!("resume failed: {}", e)).with_request_id(rid.clone())
        })?;

    let interrupts: Vec<Value> = result
        .interrupt
        .as_ref()
        .map(|v| v.iter().map(|i| i.value.clone()).collect())
        .unwrap_or_default();
    let status = if interrupts.is_empty() {
        "completed".to_string()
    } else {
        "interrupted".to_string()
    };

    #[cfg(feature = "sqlite-persistence")]
    if let Some(repo) = state.runtime_repo.as_ref() {
        let _ = repo.upsert_job(&thread_id, &status);
        let pending = repo
            .list_interrupts(Some("pending"), Some(&thread_id), 100)
            .unwrap_or_default();
        for row in pending {
            let _ = repo.update_interrupt_status(&row.interrupt_id, "resumed");
        }
        if !interrupts.is_empty() {
            let attempt_id = format!("attempt-{}-main", thread_id);
            for (i, iv) in interrupts.iter().enumerate() {
                let interrupt_id = format!("int-{}-{}", thread_id, i);
                let value_json = serde_json::to_string(iv).unwrap_or_default();
                let _ = repo.insert_interrupt(
                    &interrupt_id,
                    &thread_id,
                    &thread_id,
                    &attempt_id,
                    &value_json,
                );
            }
        }
    }

    Ok(Json(ApiEnvelope {
        meta: ApiMeta::ok(),
        request_id: rid,
        data: RunJobResponse {
            thread_id,
            status,
            interrupts,
            idempotency_key: None,
            idempotent_replay: false,
        },
    }))
}

pub async fn replay_job(
    State(state): State<ExecutionApiState>,
    Path(thread_id): Path<String>,
    headers: HeaderMap,
    Json(req): Json<ReplayJobRequest>,
) -> Result<Json<ApiEnvelope<RunJobResponse>>, ApiError> {
    let rid = request_id(&headers);
    validate_thread_id(&thread_id).map_err(|e| e.with_request_id(rid.clone()))?;
    ensure_not_cancelled(&state, &thread_id)
        .await
        .map_err(|e| e.with_request_id(rid.clone()))?;

    let config = if let Some(cp) = req.checkpoint_id.as_ref() {
        RunnableConfig::with_checkpoint(&thread_id, cp)
    } else {
        RunnableConfig::with_thread_id(&thread_id)
    };
    log::info!(
        "execution_replay request_id={} thread_id={} checkpoint_id={}",
        rid,
        thread_id,
        req.checkpoint_id
            .clone()
            .unwrap_or_else(|| "none".to_string())
    );

    let _state = state
        .compiled
        .invoke_with_config(None, &config)
        .await
        .map_err(|e| {
            ApiError::internal(format!("replay failed: {}", e)).with_request_id(rid.clone())
        })?;

    Ok(Json(ApiEnvelope {
        meta: ApiMeta::ok(),
        request_id: rid,
        data: RunJobResponse {
            thread_id,
            status: "completed".to_string(),
            interrupts: Vec::new(),
            idempotency_key: None,
            idempotent_replay: false,
        },
    }))
}

pub async fn cancel_job(
    State(state): State<ExecutionApiState>,
    Path(thread_id): Path<String>,
    headers: HeaderMap,
    Json(req): Json<CancelJobRequest>,
) -> Result<Json<ApiEnvelope<CancelJobResponse>>, ApiError> {
    let rid = request_id(&headers);
    validate_thread_id(&thread_id).map_err(|e| e.with_request_id(rid.clone()))?;
    log::info!(
        "execution_cancel request_id={} thread_id={} checkpoint_id=none",
        rid,
        thread_id
    );
    state
        .cancelled_threads
        .write()
        .await
        .insert(thread_id.clone());
    Ok(Json(ApiEnvelope {
        meta: ApiMeta::ok(),
        request_id: rid,
        data: CancelJobResponse {
            thread_id,
            status: "cancelled".to_string(),
            reason: req.reason,
        },
    }))
}

#[derive(serde::Deserialize)]
pub struct ListJobsQuery {
    pub status: Option<String>,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}

pub async fn list_jobs(
    State(state): State<ExecutionApiState>,
    headers: HeaderMap,
    Query(q): Query<ListJobsQuery>,
) -> Result<Json<ApiEnvelope<ListJobsResponse>>, ApiError> {
    let rid = request_id(&headers);
    #[cfg(feature = "sqlite-persistence")]
    {
        let repo = runtime_repo(&state, &rid)?.clone();
        let limit = q.limit.unwrap_or(50).min(200);
        let offset = q.offset.unwrap_or(0);
        let status_filter = q.status.as_deref();
        let runs = repo
            .list_runs(limit, offset, status_filter)
            .map_err(|e| ApiError::internal(e.to_string()).with_request_id(rid.clone()))?;
        let jobs = runs
            .into_iter()
            .map(|(tid, st, updated)| JobListItem {
                thread_id: tid,
                status: st,
                updated_at: updated.to_rfc3339(),
            })
            .collect();
        return Ok(Json(ApiEnvelope {
            meta: ApiMeta::ok(),
            request_id: rid,
            data: ListJobsResponse { jobs },
        }));
    }
    #[cfg(not(feature = "sqlite-persistence"))]
    {
        let _ = q;
        Ok(Json(ApiEnvelope {
            meta: ApiMeta::ok(),
            request_id: rid,
            data: ListJobsResponse { jobs: vec![] },
        }))
    }
}

#[derive(serde::Deserialize)]
pub struct ListInterruptsQuery {
    pub status: Option<String>,
    pub run_id: Option<String>,
    pub limit: Option<usize>,
}

pub async fn list_interrupts(
    State(state): State<ExecutionApiState>,
    headers: HeaderMap,
    Query(q): Query<ListInterruptsQuery>,
) -> Result<Json<ApiEnvelope<InterruptListResponse>>, ApiError> {
    let rid = request_id(&headers);
    #[cfg(feature = "sqlite-persistence")]
    {
        let repo = runtime_repo(&state, &rid)?.clone();
        let limit = q.limit.unwrap_or(50).min(200);
        let rows = repo
            .list_interrupts(q.status.as_deref(), q.run_id.as_deref(), limit)
            .map_err(|e| ApiError::internal(e.to_string()).with_request_id(rid.clone()))?;
        let interrupts = rows
            .into_iter()
            .map(|r| {
                let value = serde_json::from_str(&r.value_json).unwrap_or(Value::Null);
                InterruptListItem {
                    interrupt_id: r.interrupt_id,
                    thread_id: r.thread_id,
                    run_id: r.run_id,
                    value,
                    status: r.status,
                    created_at: r.created_at.to_rfc3339(),
                }
            })
            .collect();
        return Ok(Json(ApiEnvelope {
            meta: ApiMeta::ok(),
            request_id: rid,
            data: InterruptListResponse { interrupts },
        }));
    }
    #[cfg(not(feature = "sqlite-persistence"))]
    {
        let _ = q;
        Ok(Json(ApiEnvelope {
            meta: ApiMeta::ok(),
            request_id: rid,
            data: InterruptListResponse { interrupts: vec![] },
        }))
    }
}

pub async fn get_interrupt(
    State(state): State<ExecutionApiState>,
    Path(interrupt_id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<ApiEnvelope<InterruptDetailResponse>>, ApiError> {
    let rid = request_id(&headers);
    #[cfg(feature = "sqlite-persistence")]
    {
        let repo = runtime_repo(&state, &rid)?.clone();
        let row = repo
            .get_interrupt(&interrupt_id)
            .map_err(|e| ApiError::internal(e.to_string()).with_request_id(rid.clone()))?
            .ok_or_else(|| {
                ApiError::not_found("interrupt not found").with_request_id(rid.clone())
            })?;
        let value = serde_json::from_str(&row.value_json).unwrap_or(Value::Null);
        return Ok(Json(ApiEnvelope {
            meta: ApiMeta::ok(),
            request_id: rid,
            data: InterruptDetailResponse {
                interrupt_id: row.interrupt_id,
                thread_id: row.thread_id,
                run_id: row.run_id,
                attempt_id: row.attempt_id,
                value,
                status: row.status,
                created_at: row.created_at.to_rfc3339(),
            },
        }));
    }
    #[cfg(not(feature = "sqlite-persistence"))]
    {
        let _ = interrupt_id;
        Err(ApiError::internal("interrupt API requires sqlite-persistence").with_request_id(rid))
    }
}

pub async fn resume_interrupt(
    State(state): State<ExecutionApiState>,
    Path(interrupt_id): Path<String>,
    headers: HeaderMap,
    Json(req): Json<ResumeInterruptRequest>,
) -> Result<Json<ApiEnvelope<RunJobResponse>>, ApiError> {
    let rid = request_id(&headers);
    #[cfg(feature = "sqlite-persistence")]
    {
        let repo = runtime_repo(&state, &rid)?.clone();
        let resume_hash = json_hash(&req.value).map_err(|e| e.with_request_id(rid.clone()))?;
        let row = repo
            .get_interrupt(&interrupt_id)
            .map_err(|e| ApiError::internal(e.to_string()).with_request_id(rid.clone()))?
            .ok_or_else(|| {
                ApiError::not_found("interrupt not found").with_request_id(rid.clone())
            })?;

        if row.status == "resumed" {
            if row.resume_payload_hash.as_deref() != Some(resume_hash.as_str()) {
                return Err(
                    ApiError::conflict("interrupt already resumed with different payload")
                        .with_request_id(rid.clone()),
                );
            }
            let response_json = row.resume_response_json.ok_or_else(|| {
                ApiError::internal("missing stored resume response").with_request_id(rid.clone())
            })?;
            let response: RunJobResponse = serde_json::from_str(&response_json).map_err(|e| {
                ApiError::internal(format!("decode stored resume response failed: {}", e))
                    .with_request_id(rid.clone())
            })?;
            return Ok(Json(ApiEnvelope {
                meta: ApiMeta::ok(),
                request_id: rid,
                data: response,
            }));
        }

        if row.status != "pending" {
            return Err(
                ApiError::conflict(format!("interrupt already {}", row.status))
                    .with_request_id(rid.clone()),
            );
        }
        if state
            .cancelled_threads
            .read()
            .await
            .contains(&row.thread_id)
        {
            return Err(
                ApiError::conflict(format!("thread '{}' is cancelled", row.thread_id))
                    .with_request_id(rid.clone()),
            );
        }

        repo.update_interrupt_status(&interrupt_id, "resuming")
            .map_err(|e| ApiError::internal(e.to_string()).with_request_id(rid.clone()))?;

        let resume_req = ResumeJobRequest {
            value: req.value,
            checkpoint_id: None,
        };
        let envelope = match resume_job(
            State(state),
            Path(row.thread_id.clone()),
            headers,
            Json(resume_req),
        )
        .await
        {
            Ok(response) => response.0,
            Err(err) => {
                let _ = repo.update_interrupt_status(&interrupt_id, "pending");
                return Err(err);
            }
        };
        let response_json = serde_json::to_string(&envelope.data).map_err(|e| {
            ApiError::internal(format!("encode resume response failed: {}", e))
                .with_request_id(rid.clone())
        })?;
        repo.persist_interrupt_resume_result(&interrupt_id, &resume_hash, &response_json)
            .map_err(|e| {
                let msg = e.to_string();
                if msg.contains("different payload") {
                    ApiError::conflict(msg).with_request_id(rid.clone())
                } else {
                    ApiError::internal(msg).with_request_id(rid.clone())
                }
            })?;
        return Ok(Json(envelope));
    }
    #[cfg(not(feature = "sqlite-persistence"))]
    {
        let _ = (interrupt_id, req);
        Err(ApiError::internal("interrupt API requires sqlite-persistence").with_request_id(rid))
    }
}

pub async fn reject_interrupt(
    State(state): State<ExecutionApiState>,
    Path(interrupt_id): Path<String>,
    headers: HeaderMap,
    Json(_req): Json<RejectInterruptRequest>,
) -> Result<Json<ApiEnvelope<CancelJobResponse>>, ApiError> {
    let rid = request_id(&headers);
    #[cfg(feature = "sqlite-persistence")]
    {
        let repo = runtime_repo(&state, &rid)?;
        let row = repo
            .get_interrupt(&interrupt_id)
            .map_err(|e| ApiError::internal(e.to_string()).with_request_id(rid.clone()))?
            .ok_or_else(|| {
                ApiError::not_found("interrupt not found").with_request_id(rid.clone())
            })?;
        if row.status != "pending" {
            return Err(
                ApiError::conflict(format!("interrupt already {}", row.status))
                    .with_request_id(rid.clone()),
            );
        }
        repo.update_interrupt_status(&interrupt_id, "rejected")
            .map_err(|e| ApiError::internal(e.to_string()).with_request_id(rid.clone()))?;
        state
            .cancelled_threads
            .write()
            .await
            .insert(row.thread_id.clone());
        repo.upsert_job(&row.thread_id, "cancelled")
            .map_err(|e| ApiError::internal(e.to_string()).with_request_id(rid.clone()))?;
        return Ok(Json(ApiEnvelope {
            meta: ApiMeta::ok(),
            request_id: rid,
            data: CancelJobResponse {
                thread_id: row.thread_id,
                status: "cancelled".to_string(),
                reason: Some("interrupt rejected".to_string()),
            },
        }));
    }
    #[cfg(not(feature = "sqlite-persistence"))]
    {
        let _ = interrupt_id;
        Err(ApiError::internal("interrupt API requires sqlite-persistence").with_request_id(rid))
    }
}

pub async fn job_detail(
    State(state): State<ExecutionApiState>,
    Path(thread_id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<ApiEnvelope<JobDetailResponse>>, ApiError> {
    let rid = request_id(&headers);
    validate_thread_id(&thread_id).map_err(|e| e.with_request_id(rid.clone()))?;
    let config = RunnableConfig::with_thread_id(&thread_id);
    let snapshot = state.compiled.get_state(&config).await.map_err(|e| {
        if e.to_string().contains("No state found") {
            ApiError::not_found(e.to_string()).with_request_id(rid.clone())
        } else {
            ApiError::internal(format!("inspect failed: {}", e)).with_request_id(rid.clone())
        }
    })?;
    let history = state
        .compiled
        .get_state_history(&config)
        .await
        .unwrap_or_default();
    let history_items = history
        .iter()
        .map(|s| JobHistoryItem {
            checkpoint_id: s.checkpoint_id().cloned(),
            created_at: s.created_at.to_rfc3339(),
        })
        .collect();
    let timeline = history
        .iter()
        .enumerate()
        .map(|(i, s)| JobTimelineItem {
            seq: (i + 1) as u64,
            event_type: "checkpoint_saved".to_string(),
            checkpoint_id: s.checkpoint_id().cloned(),
            created_at: s.created_at.to_rfc3339(),
        })
        .collect();
    let values = serde_json::to_value(&snapshot.values).unwrap_or(Value::Null);
    let status = if state.cancelled_threads.read().await.contains(&thread_id) {
        "cancelled".to_string()
    } else {
        "running".to_string()
    };
    let pending_interrupt = {
        #[cfg(feature = "sqlite-persistence")]
        {
            state
                .runtime_repo
                .as_ref()
                .and_then(|repo| {
                    repo.list_interrupts(Some("pending"), Some(&thread_id), 1)
                        .ok()
                        .and_then(|rows| rows.into_iter().next())
                })
                .map(|r| InterruptDetailResponse {
                    interrupt_id: r.interrupt_id,
                    thread_id: r.thread_id,
                    run_id: r.run_id,
                    attempt_id: r.attempt_id,
                    value: serde_json::from_str(&r.value_json).unwrap_or(Value::Null),
                    status: r.status,
                    created_at: r.created_at.to_rfc3339(),
                })
        }
        #[cfg(not(feature = "sqlite-persistence"))]
        {
            None
        }
    };
    Ok(Json(ApiEnvelope {
        meta: ApiMeta::ok(),
        request_id: rid,
        data: JobDetailResponse {
            thread_id,
            status,
            checkpoint_id: snapshot.checkpoint_id().cloned(),
            values,
            history: history_items,
            timeline,
            pending_interrupt,
        },
    }))
}

pub async fn export_timeline(
    State(state): State<ExecutionApiState>,
    Path(thread_id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<ApiEnvelope<TimelineExportResponse>>, ApiError> {
    let rid = request_id(&headers);
    validate_thread_id(&thread_id).map_err(|e| e.with_request_id(rid.clone()))?;
    let config = RunnableConfig::with_thread_id(&thread_id);
    let history = state
        .compiled
        .get_state_history(&config)
        .await
        .map_err(|e| {
            ApiError::internal(format!("timeline export failed: {}", e))
                .with_request_id(rid.clone())
        })?;
    if history.is_empty() {
        return Err(
            ApiError::not_found(format!("No timeline found for thread: {}", thread_id))
                .with_request_id(rid.clone()),
        );
    }
    let timeline = history
        .iter()
        .enumerate()
        .map(|(i, s)| JobTimelineItem {
            seq: (i + 1) as u64,
            event_type: "checkpoint_saved".to_string(),
            checkpoint_id: s.checkpoint_id().cloned(),
            created_at: s.created_at.to_rfc3339(),
        })
        .collect();
    let history_items = history
        .iter()
        .map(|s| JobHistoryItem {
            checkpoint_id: s.checkpoint_id().cloned(),
            created_at: s.created_at.to_rfc3339(),
        })
        .collect();
    Ok(Json(ApiEnvelope {
        meta: ApiMeta::ok(),
        request_id: rid,
        data: TimelineExportResponse {
            thread_id,
            timeline,
            history: history_items,
        },
    }))
}

pub async fn worker_poll(
    State(state): State<ExecutionApiState>,
    headers: HeaderMap,
    Json(req): Json<WorkerPollRequest>,
) -> Result<Json<ApiEnvelope<WorkerPollResponse>>, ApiError> {
    let rid = request_id(&headers);
    validate_worker_id(&req.worker_id).map_err(|e| e.with_request_id(rid.clone()))?;

    #[cfg(feature = "sqlite-persistence")]
    {
        let repo = runtime_repo(&state, &rid)?;
        let poll_limit = req.limit.unwrap_or(state.worker_poll_limit).max(1);
        let max_active = req
            .max_active_leases
            .unwrap_or(state.max_active_leases_per_worker)
            .max(1);

        let lease_manager = RepositoryLeaseManager::new(repo.clone(), LeaseConfig::default());
        lease_manager
            .tick(Utc::now())
            .map_err(|e| ApiError::internal(e.to_string()).with_request_id(rid.clone()))?;

        let active = repo
            .active_leases_for_worker(&req.worker_id, Utc::now())
            .map_err(|e| ApiError::internal(e.to_string()).with_request_id(rid.clone()))?;
        if active >= max_active {
            return Ok(Json(ApiEnvelope {
                meta: ApiMeta::ok(),
                request_id: rid,
                data: WorkerPollResponse {
                    decision: "backpressure".to_string(),
                    attempt_id: None,
                    lease_id: None,
                    lease_expires_at: None,
                },
            }));
        }

        let scheduler = SkeletonScheduler::new(repo.clone());
        for _ in 0..poll_limit {
            let decision = scheduler
                .dispatch_one(&req.worker_id)
                .map_err(|e| ApiError::internal(e.to_string()).with_request_id(rid.clone()))?;
            if let SchedulerDecision::Dispatched { attempt_id, .. } = decision {
                let lease = repo
                    .get_lease_for_attempt(&attempt_id)
                    .map_err(|e| ApiError::internal(e.to_string()).with_request_id(rid.clone()))?
                    .ok_or_else(|| {
                        ApiError::internal("lease missing after dispatch")
                            .with_request_id(rid.clone())
                    })?;
                return Ok(Json(ApiEnvelope {
                    meta: ApiMeta::ok(),
                    request_id: rid,
                    data: WorkerPollResponse {
                        decision: "dispatched".to_string(),
                        attempt_id: Some(attempt_id),
                        lease_id: Some(lease.lease_id),
                        lease_expires_at: Some(lease.lease_expires_at.to_rfc3339()),
                    },
                }));
            }
        }
        return Ok(Json(ApiEnvelope {
            meta: ApiMeta::ok(),
            request_id: rid,
            data: WorkerPollResponse {
                decision: "noop".to_string(),
                attempt_id: None,
                lease_id: None,
                lease_expires_at: None,
            },
        }));
    }

    #[cfg(not(feature = "sqlite-persistence"))]
    {
        Err(ApiError::internal("worker APIs require sqlite-persistence").with_request_id(rid))
    }
}

pub async fn worker_heartbeat(
    State(state): State<ExecutionApiState>,
    Path(worker_id): Path<String>,
    headers: HeaderMap,
    Json(req): Json<WorkerHeartbeatRequest>,
) -> Result<Json<ApiEnvelope<WorkerLeaseResponse>>, ApiError> {
    let rid = request_id(&headers);
    validate_worker_id(&worker_id).map_err(|e| e.with_request_id(rid.clone()))?;

    #[cfg(feature = "sqlite-persistence")]
    {
        let repo = runtime_repo(&state, &rid)?;
        let lease = repo
            .get_lease_by_id(&req.lease_id)
            .map_err(|e| ApiError::internal(e.to_string()).with_request_id(rid.clone()))?
            .ok_or_else(|| ApiError::not_found("lease not found").with_request_id(rid.clone()))?;
        if lease.worker_id != worker_id {
            return Err(ApiError::conflict("lease ownership mismatch")
                .with_request_id(rid.clone())
                .with_details(serde_json::json!({
                    "expected_worker_id": lease.worker_id,
                    "actual_worker_id": worker_id
                })));
        }
        let ttl = req.lease_ttl_seconds.unwrap_or(30).max(1);
        let now = Utc::now();
        let expires = now + Duration::seconds(ttl);
        repo.heartbeat_lease_with_version(&req.lease_id, &worker_id, lease.version, now, expires)
            .map_err(|e| ApiError::internal(e.to_string()).with_request_id(rid.clone()))?;
        return Ok(Json(ApiEnvelope {
            meta: ApiMeta::ok(),
            request_id: rid,
            data: WorkerLeaseResponse {
                worker_id,
                lease_id: req.lease_id,
                lease_expires_at: expires.to_rfc3339(),
            },
        }));
    }

    #[cfg(not(feature = "sqlite-persistence"))]
    {
        Err(ApiError::internal("worker APIs require sqlite-persistence").with_request_id(rid))
    }
}

pub async fn worker_extend_lease(
    State(state): State<ExecutionApiState>,
    Path(worker_id): Path<String>,
    headers: HeaderMap,
    Json(req): Json<WorkerExtendLeaseRequest>,
) -> Result<Json<ApiEnvelope<WorkerLeaseResponse>>, ApiError> {
    let rid = request_id(&headers);
    validate_worker_id(&worker_id).map_err(|e| e.with_request_id(rid.clone()))?;
    let heartbeat_req = WorkerHeartbeatRequest {
        lease_id: req.lease_id,
        lease_ttl_seconds: req.lease_ttl_seconds,
    };
    worker_heartbeat(State(state), Path(worker_id), headers, Json(heartbeat_req)).await
}

pub async fn worker_report_step(
    State(state): State<ExecutionApiState>,
    Path(worker_id): Path<String>,
    headers: HeaderMap,
    Json(req): Json<WorkerReportStepRequest>,
) -> Result<Json<ApiEnvelope<WorkerAckResponse>>, ApiError> {
    let rid = request_id(&headers);
    validate_worker_id(&worker_id).map_err(|e| e.with_request_id(rid.clone()))?;
    if req.attempt_id.trim().is_empty() {
        return Err(ApiError::bad_request("attempt_id must not be empty").with_request_id(rid));
    }
    if req.action_id.trim().is_empty() {
        return Err(ApiError::bad_request("action_id must not be empty").with_request_id(rid));
    }
    if req.status.trim().is_empty() {
        return Err(ApiError::bad_request("status must not be empty").with_request_id(rid));
    }
    if req.dedupe_token.trim().is_empty() {
        return Err(ApiError::bad_request("dedupe_token must not be empty").with_request_id(rid));
    }

    #[cfg(feature = "sqlite-persistence")]
    let report_status = {
        let repo = runtime_repo(&state, &rid)?;
        match repo.record_step_report(
            &worker_id,
            &req.attempt_id,
            &req.action_id,
            &req.status,
            &req.dedupe_token,
        ) {
            Ok(StepReportWriteResult::Inserted) => "reported".to_string(),
            Ok(StepReportWriteResult::Duplicate) => "reported_idempotent".to_string(),
            Err(e) => {
                let msg = e.to_string();
                if msg.contains("dedupe_token") {
                    return Err(ApiError::conflict(msg).with_request_id(rid));
                }
                return Err(ApiError::internal(msg).with_request_id(rid));
            }
        }
    };

    #[cfg(not(feature = "sqlite-persistence"))]
    let report_status = {
        let _ = state;
        "reported".to_string()
    };

    Ok(Json(ApiEnvelope {
        meta: ApiMeta::ok(),
        request_id: rid,
        data: WorkerAckResponse {
            attempt_id: req.attempt_id,
            status: report_status,
        },
    }))
}

pub async fn worker_ack(
    State(state): State<ExecutionApiState>,
    Path(worker_id): Path<String>,
    headers: HeaderMap,
    Json(req): Json<WorkerAckRequest>,
) -> Result<Json<ApiEnvelope<WorkerAckResponse>>, ApiError> {
    let rid = request_id(&headers);
    validate_worker_id(&worker_id).map_err(|e| e.with_request_id(rid.clone()))?;
    if req.attempt_id.trim().is_empty() {
        return Err(ApiError::bad_request("attempt_id must not be empty").with_request_id(rid));
    }

    #[cfg(feature = "sqlite-persistence")]
    {
        let repo = runtime_repo(&state, &rid)?;
        let status = match req.terminal_status.as_str() {
            "completed" => AttemptExecutionStatus::Completed,
            "failed" => AttemptExecutionStatus::Failed,
            "cancelled" => AttemptExecutionStatus::Cancelled,
            _ => {
                return Err(ApiError::bad_request(
                    "terminal_status must be one of: completed|failed|cancelled",
                )
                .with_request_id(rid))
            }
        };
        repo.mark_attempt_status(&req.attempt_id, status)
            .map_err(|e| ApiError::internal(e.to_string()).with_request_id(rid.clone()))?;
        return Ok(Json(ApiEnvelope {
            meta: ApiMeta::ok(),
            request_id: rid,
            data: WorkerAckResponse {
                attempt_id: req.attempt_id,
                status: req.terminal_status,
            },
        }));
    }

    #[cfg(not(feature = "sqlite-persistence"))]
    {
        Err(ApiError::internal("worker APIs require sqlite-persistence").with_request_id(rid))
    }
}

#[cfg(all(test, feature = "execution-server"))]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;

    use axum::body::Body;
    use axum::http::{Method, Request, StatusCode};
    use chrono::{Duration, Utc};
    use tower::util::ServiceExt;

    use crate::graph::{
        function_node, interrupt, GraphError, InMemorySaver, MessagesState, StateGraph, END, START,
    };
    use crate::kernel::runtime::repository::RuntimeRepository;
    use crate::schemas::messages::Message;

    use super::{build_router, ExecutionApiState};

    async fn build_test_graph() -> Arc<crate::graph::CompiledGraph<MessagesState>> {
        let node = function_node("research", |_state: &MessagesState| async move {
            let mut update = HashMap::new();
            update.insert(
                "messages".to_string(),
                serde_json::to_value(vec![Message::new_ai_message("ok")]).unwrap(),
            );
            Ok(update)
        });
        let mut graph = StateGraph::<MessagesState>::new();
        graph.add_node("research", node).unwrap();
        graph.add_edge(START, "research");
        graph.add_edge("research", END);
        let saver = Arc::new(InMemorySaver::new());
        Arc::new(graph.compile_with_persistence(Some(saver), None).unwrap())
    }

    async fn build_interrupt_graph() -> Arc<crate::graph::CompiledGraph<MessagesState>> {
        let node = function_node("approval", |_state: &MessagesState| async move {
            let approved = interrupt("approve?")
                .await
                .map_err(GraphError::InterruptError)?;
            let mut update = HashMap::new();
            update.insert(
                "messages".to_string(),
                serde_json::to_value(vec![Message::new_ai_message(format!(
                    "approved={}",
                    approved
                ))])
                .unwrap(),
            );
            Ok(update)
        });
        let mut graph = StateGraph::<MessagesState>::new();
        graph.add_node("approval", node).unwrap();
        graph.add_edge(START, "approval");
        graph.add_edge("approval", END);
        let saver = Arc::new(InMemorySaver::new());
        Arc::new(graph.compile_with_persistence(Some(saver), None).unwrap())
    }

    #[tokio::test]
    async fn run_and_inspect_path_works() {
        let router = build_router(ExecutionApiState::new(build_interrupt_graph().await));

        let run_req = Request::builder()
            .method(Method::POST)
            .uri("/v1/jobs/run")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "thread_id": "api-test-1",
                    "input": "hello"
                })
                .to_string(),
            ))
            .unwrap();
        let run_resp = router.clone().oneshot(run_req).await.unwrap();
        assert_eq!(run_resp.status(), StatusCode::OK);

        let inspect_req = Request::builder()
            .method(Method::GET)
            .uri("/v1/jobs/api-test-1")
            .body(Body::empty())
            .unwrap();
        let inspect_resp = router.clone().oneshot(inspect_req).await.unwrap();
        assert_eq!(inspect_resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn empty_thread_id_is_bad_request() {
        let router = build_router(ExecutionApiState::new(build_test_graph().await));
        let req = Request::builder()
            .method(Method::POST)
            .uri("/v1/jobs/run")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "thread_id": "",
                })
                .to_string(),
            ))
            .unwrap();
        let resp = router.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn auth_required_without_credentials_returns_unauthorized() {
        let router = build_router(
            ExecutionApiState::new(build_test_graph().await).with_static_api_key("test-api-key"),
        );
        let req = Request::builder()
            .method(Method::POST)
            .uri("/v1/jobs/run")
            .header("x-request-id", "req-auth-1")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "thread_id": "auth-run-1"
                })
                .to_string(),
            ))
            .unwrap();
        let resp = router.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .expect("auth body");
        let json: serde_json::Value = serde_json::from_slice(&body).expect("auth json");
        assert_eq!(json["request_id"], "req-auth-1");
        assert_eq!(json["error"]["code"], "unauthorized");
    }

    #[tokio::test]
    async fn auth_bearer_token_allows_access() {
        let router = build_router(
            ExecutionApiState::new(build_test_graph().await).with_static_bearer_token("t-1"),
        );
        let req = Request::builder()
            .method(Method::POST)
            .uri("/v1/jobs/run")
            .header("authorization", "Bearer t-1")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "thread_id": "auth-run-2"
                })
                .to_string(),
            ))
            .unwrap();
        let resp = router.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn auth_api_key_allows_access() {
        let router = build_router(
            ExecutionApiState::new(build_test_graph().await).with_static_api_key("key-1"),
        );
        let req = Request::builder()
            .method(Method::POST)
            .uri("/v1/jobs/run")
            .header("x-api-key", "key-1")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "thread_id": "auth-run-3"
                })
                .to_string(),
            ))
            .unwrap();
        let resp = router.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn auth_keyed_api_key_allows_access() {
        let router = build_router(
            ExecutionApiState::new(build_test_graph().await).with_static_api_key_record(
                "ops-key-1",
                "secret-1",
                true,
            ),
        );
        let req = Request::builder()
            .method(Method::POST)
            .uri("/v1/jobs/run")
            .header("x-api-key-id", "ops-key-1")
            .header("x-api-key", "secret-1")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "thread_id": "auth-run-4"
                })
                .to_string(),
            ))
            .unwrap();
        let resp = router.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn auth_keyed_api_key_disabled_is_rejected() {
        let router = build_router(
            ExecutionApiState::new(build_test_graph().await).with_static_api_key_record(
                "ops-key-2",
                "secret-2",
                false,
            ),
        );
        let req = Request::builder()
            .method(Method::POST)
            .uri("/v1/jobs/run")
            .header("x-api-key-id", "ops-key-2")
            .header("x-api-key", "secret-2")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "thread_id": "auth-run-5"
                })
                .to_string(),
            ))
            .unwrap();
        let resp = router.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn auth_keyed_api_key_wrong_secret_is_rejected() {
        let router = build_router(
            ExecutionApiState::new(build_test_graph().await).with_static_api_key_record(
                "ops-key-3",
                "secret-3",
                true,
            ),
        );
        let req = Request::builder()
            .method(Method::POST)
            .uri("/v1/jobs/run")
            .header("x-api-key-id", "ops-key-3")
            .header("x-api-key", "secret-wrong")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "thread_id": "auth-run-6"
                })
                .to_string(),
            ))
            .unwrap();
        let resp = router.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn e2e_run_history_resume_inspect() {
        let router = build_router(ExecutionApiState::new(build_interrupt_graph().await));

        let run_req = Request::builder()
            .method(Method::POST)
            .uri("/v1/jobs/run")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "thread_id": "api-e2e-1",
                    "input": "trigger interrupt"
                })
                .to_string(),
            ))
            .unwrap();
        let run_resp = router.clone().oneshot(run_req).await.unwrap();
        assert_eq!(run_resp.status(), StatusCode::OK);

        let history_req = Request::builder()
            .method(Method::GET)
            .uri("/v1/jobs/api-e2e-1/history")
            .body(Body::empty())
            .unwrap();
        let history_resp = router.clone().oneshot(history_req).await.unwrap();
        assert_eq!(history_resp.status(), StatusCode::OK);

        let resume_req = Request::builder()
            .method(Method::POST)
            .uri("/v1/jobs/api-e2e-1/resume")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "value": true
                })
                .to_string(),
            ))
            .unwrap();
        let resume_resp = router.clone().oneshot(resume_req).await.unwrap();
        assert_eq!(resume_resp.status(), StatusCode::OK);

        let inspect_req = Request::builder()
            .method(Method::GET)
            .uri("/v1/jobs/api-e2e-1")
            .body(Body::empty())
            .unwrap();
        let inspect_resp = router.clone().oneshot(inspect_req).await.unwrap();
        assert_eq!(inspect_resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn cancel_then_run_returns_conflict() {
        let router = build_router(ExecutionApiState::new(build_test_graph().await));
        let cancel_req = Request::builder()
            .method(Method::POST)
            .uri("/v1/jobs/cancelled-1/cancel")
            .header("content-type", "application/json")
            .body(Body::from("{}"))
            .unwrap();
        let cancel_resp = router.clone().oneshot(cancel_req).await.unwrap();
        assert_eq!(cancel_resp.status(), StatusCode::OK);

        let run_req = Request::builder()
            .method(Method::POST)
            .uri("/v1/jobs/run")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "thread_id": "cancelled-1",
                    "input": "no-op"
                })
                .to_string(),
            ))
            .unwrap();
        let run_resp = router.oneshot(run_req).await.unwrap();
        assert_eq!(run_resp.status(), StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn timeline_and_checkpoint_inspect_work() {
        let router = build_router(ExecutionApiState::new(build_interrupt_graph().await));
        let run_req = Request::builder()
            .method(Method::POST)
            .uri("/v1/jobs/run")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "thread_id": "api-timeline-1",
                    "input": "hello"
                })
                .to_string(),
            ))
            .unwrap();
        let run_resp = router.clone().oneshot(run_req).await.unwrap();
        assert_eq!(run_resp.status(), StatusCode::OK);

        let timeline_req = Request::builder()
            .method(Method::GET)
            .uri("/v1/jobs/api-timeline-1/timeline")
            .body(Body::empty())
            .unwrap();
        let timeline_resp = router.clone().oneshot(timeline_req).await.unwrap();
        assert_eq!(timeline_resp.status(), StatusCode::OK);

        let history_req = Request::builder()
            .method(Method::GET)
            .uri("/v1/jobs/api-timeline-1/history")
            .body(Body::empty())
            .unwrap();
        let history_resp = router.clone().oneshot(history_req).await.unwrap();
        let history_body = axum::body::to_bytes(history_resp.into_body(), usize::MAX)
            .await
            .expect("history body");
        let history_json: serde_json::Value =
            serde_json::from_slice(&history_body).expect("history json");
        let checkpoint_id = history_json["data"]["history"][0]["checkpoint_id"]
            .as_str()
            .expect("checkpoint_id")
            .to_string();

        let checkpoint_req = Request::builder()
            .method(Method::GET)
            .uri(format!(
                "/v1/jobs/api-timeline-1/checkpoints/{}",
                checkpoint_id
            ))
            .body(Body::empty())
            .unwrap();
        let checkpoint_resp = router.oneshot(checkpoint_req).await.unwrap();
        assert_eq!(checkpoint_resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn checkpoint_inspect_invalid_checkpoint_is_not_found() {
        let router = build_router(ExecutionApiState::new(build_test_graph().await));
        let req = Request::builder()
            .method(Method::GET)
            .uri("/v1/jobs/no-run/checkpoints/no-checkpoint")
            .body(Body::empty())
            .unwrap();
        let resp = router.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn timeline_missing_thread_returns_not_found() {
        let router = build_router(ExecutionApiState::new(build_test_graph().await));
        let req = Request::builder()
            .method(Method::GET)
            .uri("/v1/jobs/no-timeline/timeline")
            .body(Body::empty())
            .unwrap();
        let resp = router.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn error_contract_contains_request_id_and_code() {
        let router = build_router(ExecutionApiState::new(build_test_graph().await));
        let req = Request::builder()
            .method(Method::POST)
            .uri("/v1/jobs/run")
            .header("x-request-id", "req-123")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "thread_id": ""
                })
                .to_string(),
            ))
            .unwrap();
        let resp = router.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .expect("error body");
        let json: serde_json::Value = serde_json::from_slice(&body).expect("error json");
        assert_eq!(json["request_id"], "req-123");
        assert_eq!(json["error"]["code"], "invalid_argument");
    }

    #[cfg(feature = "sqlite-persistence")]
    #[tokio::test]
    async fn idempotent_run_same_key_replays_response() {
        let router = build_router(ExecutionApiState::with_sqlite_idempotency(
            build_interrupt_graph().await,
            ":memory:",
        ));
        let body = serde_json::json!({
            "thread_id": "idem-run-1",
            "input": "hello",
            "idempotency_key": "idem-key-1"
        })
        .to_string();

        let req1 = Request::builder()
            .method(Method::POST)
            .uri("/v1/jobs/run")
            .header("content-type", "application/json")
            .body(Body::from(body.clone()))
            .unwrap();
        let resp1 = router.clone().oneshot(req1).await.unwrap();
        assert_eq!(resp1.status(), StatusCode::OK);

        let req2 = Request::builder()
            .method(Method::POST)
            .uri("/v1/jobs/run")
            .header("content-type", "application/json")
            .body(Body::from(body))
            .unwrap();
        let resp2 = router.oneshot(req2).await.unwrap();
        assert_eq!(resp2.status(), StatusCode::OK);
        let body2 = axum::body::to_bytes(resp2.into_body(), usize::MAX)
            .await
            .expect("idempotent body");
        let json2: serde_json::Value = serde_json::from_slice(&body2).expect("idempotent json");
        assert_eq!(json2["data"]["idempotent_replay"], true);
    }

    #[cfg(feature = "sqlite-persistence")]
    #[tokio::test]
    async fn idempotent_run_payload_mismatch_conflicts() {
        let router = build_router(ExecutionApiState::with_sqlite_idempotency(
            build_interrupt_graph().await,
            ":memory:",
        ));

        let req1 = Request::builder()
            .method(Method::POST)
            .uri("/v1/jobs/run")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "thread_id": "idem-run-2",
                    "input": "hello-a",
                    "idempotency_key": "idem-key-2"
                })
                .to_string(),
            ))
            .unwrap();
        let resp1 = router.clone().oneshot(req1).await.unwrap();
        assert_eq!(resp1.status(), StatusCode::OK);

        let req2 = Request::builder()
            .method(Method::POST)
            .uri("/v1/jobs/run")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "thread_id": "idem-run-2",
                    "input": "hello-b",
                    "idempotency_key": "idem-key-2"
                })
                .to_string(),
            ))
            .unwrap();
        let resp2 = router.oneshot(req2).await.unwrap();
        assert_eq!(resp2.status(), StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn post_jobs_normative_route_works() {
        let router = build_router(ExecutionApiState::new(build_interrupt_graph().await));
        let req = Request::builder()
            .method(Method::POST)
            .uri("/v1/jobs")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "thread_id": "api-post-jobs-1",
                    "input": "hello"
                })
                .to_string(),
            ))
            .unwrap();
        let resp = router.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[cfg(feature = "sqlite-persistence")]
    #[tokio::test]
    async fn duplicate_resume_same_payload_returns_same_result() {
        let router = build_router(ExecutionApiState::with_sqlite_idempotency(
            build_interrupt_graph().await,
            ":memory:",
        ));
        let run_req = Request::builder()
            .method(Method::POST)
            .uri("/v1/jobs/run")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "thread_id": "resume-idem-1",
                    "input": "trigger interrupt"
                })
                .to_string(),
            ))
            .unwrap();
        let run_resp = router.clone().oneshot(run_req).await.unwrap();
        assert_eq!(run_resp.status(), StatusCode::OK);
        let interrupt_id = "int-resume-idem-1-0";

        let first_req = Request::builder()
            .method(Method::POST)
            .uri(format!("/v1/interrupts/{}/resume", interrupt_id))
            .header("content-type", "application/json")
            .body(Body::from(serde_json::json!({ "value": true }).to_string()))
            .unwrap();
        let first_resp = router.clone().oneshot(first_req).await.unwrap();
        assert_eq!(first_resp.status(), StatusCode::OK);
        let first_body = axum::body::to_bytes(first_resp.into_body(), usize::MAX)
            .await
            .expect("first resume body");
        let first_json: serde_json::Value =
            serde_json::from_slice(&first_body).expect("first resume json");

        let second_req = Request::builder()
            .method(Method::POST)
            .uri(format!("/v1/interrupts/{}/resume", interrupt_id))
            .header("content-type", "application/json")
            .body(Body::from(serde_json::json!({ "value": true }).to_string()))
            .unwrap();
        let second_resp = router.oneshot(second_req).await.unwrap();
        assert_eq!(second_resp.status(), StatusCode::OK);
        let second_body = axum::body::to_bytes(second_resp.into_body(), usize::MAX)
            .await
            .expect("second resume body");
        let second_json: serde_json::Value =
            serde_json::from_slice(&second_body).expect("second resume json");
        assert_eq!(first_json["data"], second_json["data"]);
    }

    #[cfg(feature = "sqlite-persistence")]
    #[tokio::test]
    async fn duplicate_resume_different_payload_conflicts() {
        let router = build_router(ExecutionApiState::with_sqlite_idempotency(
            build_interrupt_graph().await,
            ":memory:",
        ));
        let run_req = Request::builder()
            .method(Method::POST)
            .uri("/v1/jobs/run")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "thread_id": "resume-idem-2",
                    "input": "trigger interrupt"
                })
                .to_string(),
            ))
            .unwrap();
        let run_resp = router.clone().oneshot(run_req).await.unwrap();
        assert_eq!(run_resp.status(), StatusCode::OK);
        let interrupt_id = "int-resume-idem-2-0";

        let first_req = Request::builder()
            .method(Method::POST)
            .uri(format!("/v1/interrupts/{}/resume", interrupt_id))
            .header("content-type", "application/json")
            .body(Body::from(serde_json::json!({ "value": true }).to_string()))
            .unwrap();
        let first_resp = router.clone().oneshot(first_req).await.unwrap();
        assert_eq!(first_resp.status(), StatusCode::OK);

        let second_req = Request::builder()
            .method(Method::POST)
            .uri(format!("/v1/interrupts/{}/resume", interrupt_id))
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({ "value": false }).to_string(),
            ))
            .unwrap();
        let second_resp = router.oneshot(second_req).await.unwrap();
        assert_eq!(second_resp.status(), StatusCode::CONFLICT);
    }

    #[cfg(feature = "sqlite-persistence")]
    #[tokio::test]
    async fn worker_report_step_dedupe_is_enforced() {
        let router = build_router(ExecutionApiState::with_sqlite_idempotency(
            build_test_graph().await,
            ":memory:",
        ));
        let req1 = Request::builder()
            .method(Method::POST)
            .uri("/v1/workers/worker-3/report-step")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "attempt_id": "attempt-report-1",
                    "action_id": "action-1",
                    "status": "succeeded",
                    "dedupe_token": "tok-1"
                })
                .to_string(),
            ))
            .unwrap();
        let resp1 = router.clone().oneshot(req1).await.unwrap();
        assert_eq!(resp1.status(), StatusCode::OK);
        let body1 = axum::body::to_bytes(resp1.into_body(), usize::MAX)
            .await
            .expect("report 1 body");
        let json1: serde_json::Value = serde_json::from_slice(&body1).expect("report 1 json");
        assert_eq!(json1["data"]["status"], "reported");

        let req2 = Request::builder()
            .method(Method::POST)
            .uri("/v1/workers/worker-3/report-step")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "attempt_id": "attempt-report-1",
                    "action_id": "action-1",
                    "status": "succeeded",
                    "dedupe_token": "tok-1"
                })
                .to_string(),
            ))
            .unwrap();
        let resp2 = router.clone().oneshot(req2).await.unwrap();
        assert_eq!(resp2.status(), StatusCode::OK);
        let body2 = axum::body::to_bytes(resp2.into_body(), usize::MAX)
            .await
            .expect("report 2 body");
        let json2: serde_json::Value = serde_json::from_slice(&body2).expect("report 2 json");
        assert_eq!(json2["data"]["status"], "reported_idempotent");

        let req3 = Request::builder()
            .method(Method::POST)
            .uri("/v1/workers/worker-3/report-step")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "attempt_id": "attempt-report-1",
                    "action_id": "action-1",
                    "status": "failed",
                    "dedupe_token": "tok-1"
                })
                .to_string(),
            ))
            .unwrap();
        let resp3 = router.oneshot(req3).await.unwrap();
        assert_eq!(resp3.status(), StatusCode::CONFLICT);
    }

    #[cfg(feature = "sqlite-persistence")]
    #[tokio::test]
    async fn worker_poll_heartbeat_ack_flow_works() {
        let state =
            ExecutionApiState::with_sqlite_idempotency(build_test_graph().await, ":memory:");
        let repo = state.runtime_repo.clone().expect("runtime repo");
        repo.enqueue_attempt("attempt-worker-1", "run-worker-1")
            .expect("enqueue");
        let router = build_router(state);

        let poll_req = Request::builder()
            .method(Method::POST)
            .uri("/v1/workers/poll")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "worker_id": "worker-1"
                })
                .to_string(),
            ))
            .unwrap();
        let poll_resp = router.clone().oneshot(poll_req).await.unwrap();
        assert_eq!(poll_resp.status(), StatusCode::OK);
        let poll_body = axum::body::to_bytes(poll_resp.into_body(), usize::MAX)
            .await
            .expect("poll body");
        let poll_json: serde_json::Value = serde_json::from_slice(&poll_body).expect("poll json");
        assert_eq!(poll_json["data"]["decision"], "dispatched");
        let lease_id = poll_json["data"]["lease_id"]
            .as_str()
            .expect("lease_id")
            .to_string();

        let hb_req = Request::builder()
            .method(Method::POST)
            .uri("/v1/workers/worker-1/heartbeat")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "lease_id": lease_id,
                    "lease_ttl_seconds": 10
                })
                .to_string(),
            ))
            .unwrap();
        let hb_resp = router.clone().oneshot(hb_req).await.unwrap();
        assert_eq!(hb_resp.status(), StatusCode::OK);

        let ack_req = Request::builder()
            .method(Method::POST)
            .uri("/v1/workers/worker-1/ack")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "attempt_id": "attempt-worker-1",
                    "terminal_status": "completed"
                })
                .to_string(),
            ))
            .unwrap();
        let ack_resp = router.oneshot(ack_req).await.unwrap();
        assert_eq!(ack_resp.status(), StatusCode::OK);
    }

    #[cfg(feature = "sqlite-persistence")]
    #[tokio::test]
    async fn worker_conflict_failover_backpressure_are_enforced() {
        let state =
            ExecutionApiState::with_sqlite_idempotency(build_test_graph().await, ":memory:");
        let repo = state.runtime_repo.clone().expect("runtime repo");
        repo.enqueue_attempt("attempt-worker-2a", "run-worker-2")
            .expect("enqueue");
        repo.enqueue_attempt("attempt-worker-2b", "run-worker-2")
            .expect("enqueue");
        let router = build_router(state);

        let first_poll_req = Request::builder()
            .method(Method::POST)
            .uri("/v1/workers/poll")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "worker_id": "worker-2",
                    "max_active_leases": 1
                })
                .to_string(),
            ))
            .unwrap();
        let first_poll_resp = router.clone().oneshot(first_poll_req).await.unwrap();
        assert_eq!(first_poll_resp.status(), StatusCode::OK);
        let first_poll_body = axum::body::to_bytes(first_poll_resp.into_body(), usize::MAX)
            .await
            .expect("first poll body");
        let first_poll_json: serde_json::Value =
            serde_json::from_slice(&first_poll_body).expect("first poll json");
        let lease_id = first_poll_json["data"]["lease_id"]
            .as_str()
            .expect("lease_id")
            .to_string();

        let backpressure_req = Request::builder()
            .method(Method::POST)
            .uri("/v1/workers/poll")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "worker_id": "worker-2",
                    "max_active_leases": 1
                })
                .to_string(),
            ))
            .unwrap();
        let backpressure_resp = router.clone().oneshot(backpressure_req).await.unwrap();
        assert_eq!(backpressure_resp.status(), StatusCode::OK);
        let backpressure_body = axum::body::to_bytes(backpressure_resp.into_body(), usize::MAX)
            .await
            .expect("backpressure body");
        let backpressure_json: serde_json::Value =
            serde_json::from_slice(&backpressure_body).expect("backpressure json");
        assert_eq!(backpressure_json["data"]["decision"], "backpressure");

        let wrong_hb_req = Request::builder()
            .method(Method::POST)
            .uri("/v1/workers/worker-other/heartbeat")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "lease_id": lease_id,
                    "lease_ttl_seconds": 5
                })
                .to_string(),
            ))
            .unwrap();
        let wrong_hb_resp = router.clone().oneshot(wrong_hb_req).await.unwrap();
        assert_eq!(wrong_hb_resp.status(), StatusCode::CONFLICT);

        repo.heartbeat_lease(
            &lease_id,
            Utc::now() - Duration::seconds(40),
            Utc::now() - Duration::seconds(20),
        )
        .expect("force-expire lease");

        let failover_poll_req = Request::builder()
            .method(Method::POST)
            .uri("/v1/workers/poll")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "worker_id": "worker-2",
                    "max_active_leases": 1
                })
                .to_string(),
            ))
            .unwrap();
        let failover_poll_resp = router.oneshot(failover_poll_req).await.unwrap();
        assert_eq!(failover_poll_resp.status(), StatusCode::OK);
        let failover_body = axum::body::to_bytes(failover_poll_resp.into_body(), usize::MAX)
            .await
            .expect("failover body");
        let failover_json: serde_json::Value =
            serde_json::from_slice(&failover_body).expect("failover json");
        assert_eq!(failover_json["data"]["decision"], "dispatched");
    }

    #[cfg(feature = "sqlite-persistence")]
    #[tokio::test]
    async fn list_jobs_empty() {
        let router = build_router(ExecutionApiState::with_sqlite_idempotency(
            build_test_graph().await,
            ":memory:",
        ));
        let req = Request::builder()
            .method(Method::GET)
            .uri("/v1/jobs")
            .body(Body::empty())
            .unwrap();
        let resp = router.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .expect("list jobs body");
        let json: serde_json::Value = serde_json::from_slice(&body).expect("list jobs json");
        assert!(json["data"]["jobs"].as_array().unwrap().is_empty());
    }

    #[cfg(feature = "sqlite-persistence")]
    #[tokio::test]
    async fn list_jobs_paginated() {
        let router = build_router(ExecutionApiState::with_sqlite_idempotency(
            build_interrupt_graph().await,
            ":memory:",
        ));
        let run_req = Request::builder()
            .method(Method::POST)
            .uri("/v1/jobs/run")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "thread_id": "list-job-1",
                    "input": "hello"
                })
                .to_string(),
            ))
            .unwrap();
        let run_resp = router.clone().oneshot(run_req).await.unwrap();
        assert_eq!(run_resp.status(), StatusCode::OK);

        let list_req = Request::builder()
            .method(Method::GET)
            .uri("/v1/jobs?limit=10&offset=0")
            .body(Body::empty())
            .unwrap();
        let list_resp = router.oneshot(list_req).await.unwrap();
        assert_eq!(list_resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(list_resp.into_body(), usize::MAX)
            .await
            .expect("list jobs body");
        let json: serde_json::Value = serde_json::from_slice(&body).expect("list jobs json");
        let jobs = json["data"]["jobs"].as_array().unwrap();
        assert!(!jobs.is_empty());
        assert_eq!(jobs[0]["thread_id"], "list-job-1");
    }

    #[cfg(feature = "sqlite-persistence")]
    #[tokio::test]
    async fn list_interrupts_filtered() {
        let router = build_router(ExecutionApiState::with_sqlite_idempotency(
            build_interrupt_graph().await,
            ":memory:",
        ));
        let run_req = Request::builder()
            .method(Method::POST)
            .uri("/v1/jobs/run")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "thread_id": "int-list-1",
                    "input": "trigger interrupt"
                })
                .to_string(),
            ))
            .unwrap();
        let run_resp = router.clone().oneshot(run_req).await.unwrap();
        assert_eq!(run_resp.status(), StatusCode::OK);

        let list_req = Request::builder()
            .method(Method::GET)
            .uri("/v1/interrupts?status=pending&run_id=int-list-1")
            .body(Body::empty())
            .unwrap();
        let list_resp = router.oneshot(list_req).await.unwrap();
        assert_eq!(list_resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(list_resp.into_body(), usize::MAX)
            .await
            .expect("list interrupts body");
        let json: serde_json::Value = serde_json::from_slice(&body).expect("list interrupts json");
        let interrupts = json["data"]["interrupts"].as_array().unwrap();
        assert!(!interrupts.is_empty());
    }

    #[cfg(feature = "sqlite-persistence")]
    #[tokio::test]
    async fn resume_interrupt_success() {
        let router = build_router(ExecutionApiState::with_sqlite_idempotency(
            build_interrupt_graph().await,
            ":memory:",
        ));
        let run_req = Request::builder()
            .method(Method::POST)
            .uri("/v1/jobs/run")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "thread_id": "resume-int-1",
                    "input": "trigger interrupt"
                })
                .to_string(),
            ))
            .unwrap();
        let run_resp = router.clone().oneshot(run_req).await.unwrap();
        assert_eq!(run_resp.status(), StatusCode::OK);
        let run_body = axum::body::to_bytes(run_resp.into_body(), usize::MAX)
            .await
            .expect("run body");
        let run_json: serde_json::Value = serde_json::from_slice(&run_body).expect("run json");
        let interrupts = run_json["data"]["interrupts"].as_array().unwrap();
        assert!(!interrupts.is_empty());
        let interrupt_id = "int-resume-int-1-0";

        let resume_req = Request::builder()
            .method(Method::POST)
            .uri(format!("/v1/interrupts/{}/resume", interrupt_id))
            .header("content-type", "application/json")
            .body(Body::from(serde_json::json!({ "value": true }).to_string()))
            .unwrap();
        let resume_resp = router.oneshot(resume_req).await.unwrap();
        assert_eq!(resume_resp.status(), StatusCode::OK);
    }

    #[cfg(feature = "sqlite-persistence")]
    #[tokio::test]
    async fn reject_interrupt() {
        let router = build_router(ExecutionApiState::with_sqlite_idempotency(
            build_interrupt_graph().await,
            ":memory:",
        ));
        let run_req = Request::builder()
            .method(Method::POST)
            .uri("/v1/jobs/run")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "thread_id": "reject-int-1",
                    "input": "trigger interrupt"
                })
                .to_string(),
            ))
            .unwrap();
        let run_resp = router.clone().oneshot(run_req).await.unwrap();
        assert_eq!(run_resp.status(), StatusCode::OK);
        let run_body = axum::body::to_bytes(run_resp.into_body(), usize::MAX)
            .await
            .expect("run body");
        let run_json: serde_json::Value = serde_json::from_slice(&run_body).expect("run json");
        let interrupts = run_json["data"]["interrupts"].as_array().unwrap();
        assert!(!interrupts.is_empty());
        let interrupt_id = "int-reject-int-1-0";

        let reject_req = Request::builder()
            .method(Method::POST)
            .uri(format!("/v1/interrupts/{}/reject", interrupt_id))
            .header("content-type", "application/json")
            .body(Body::from("{}"))
            .unwrap();
        let reject_resp = router.oneshot(reject_req).await.unwrap();
        assert_eq!(reject_resp.status(), StatusCode::OK);
    }

    #[cfg(feature = "sqlite-persistence")]
    #[tokio::test]
    async fn job_detail_works() {
        let router = build_router(ExecutionApiState::with_sqlite_idempotency(
            build_interrupt_graph().await,
            ":memory:",
        ));
        let run_req = Request::builder()
            .method(Method::POST)
            .uri("/v1/jobs/run")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "thread_id": "detail-job-1",
                    "input": "hello"
                })
                .to_string(),
            ))
            .unwrap();
        let run_resp = router.clone().oneshot(run_req).await.unwrap();
        assert_eq!(run_resp.status(), StatusCode::OK);

        let detail_req = Request::builder()
            .method(Method::GET)
            .uri("/v1/jobs/detail-job-1/detail")
            .body(Body::empty())
            .unwrap();
        let detail_resp = router.oneshot(detail_req).await.unwrap();
        assert_eq!(detail_resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(detail_resp.into_body(), usize::MAX)
            .await
            .expect("detail body");
        let json: serde_json::Value = serde_json::from_slice(&body).expect("detail json");
        assert_eq!(json["data"]["thread_id"], "detail-job-1");
    }

    #[cfg(feature = "sqlite-persistence")]
    #[tokio::test]
    async fn export_timeline_works() {
        let router = build_router(ExecutionApiState::with_sqlite_idempotency(
            build_interrupt_graph().await,
            ":memory:",
        ));
        let run_req = Request::builder()
            .method(Method::POST)
            .uri("/v1/jobs/run")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "thread_id": "export-tl-1",
                    "input": "hello"
                })
                .to_string(),
            ))
            .unwrap();
        let run_resp = router.clone().oneshot(run_req).await.unwrap();
        assert_eq!(run_resp.status(), StatusCode::OK);

        let export_req = Request::builder()
            .method(Method::GET)
            .uri("/v1/jobs/export-tl-1/timeline/export")
            .body(Body::empty())
            .unwrap();
        let export_resp = router.oneshot(export_req).await.unwrap();
        assert_eq!(export_resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(export_resp.into_body(), usize::MAX)
            .await
            .expect("export body");
        let json: serde_json::Value = serde_json::from_slice(&body).expect("export json");
        assert!(json["data"]["timeline"].is_array());
    }
}
