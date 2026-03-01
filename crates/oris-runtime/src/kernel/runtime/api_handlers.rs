//! Axum handlers for Phase 2 execution server.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use axum::extract::{Path, Query, State};
use axum::http::{
    header::{AUTHORIZATION, CONTENT_TYPE},
    HeaderMap,
};
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
    ApiEnvelope, ApiMeta, AttemptRetryHistoryItem, AttemptRetryHistoryResponse, AuditLogItem,
    AuditLogListResponse, CancelJobRequest, CancelJobResponse, CheckpointInspectResponse,
    DeadLetterItem, DeadLetterListResponse, DeadLetterReplayResponse, InterruptDetailResponse,
    InterruptListItem, InterruptListResponse, JobDetailResponse, JobHistoryItem,
    JobHistoryResponse, JobListItem, JobStateResponse, JobTimelineItem, JobTimelineResponse,
    ListAuditLogsQuery, ListDeadLettersQuery, ListJobsResponse, RejectInterruptRequest,
    ReplayJobRequest, ResumeInterruptRequest, ResumeJobRequest, RetryPolicyRequest, RunJobRequest,
    RunJobResponse, TimelineExportResponse, TimeoutPolicyRequest, TraceContextResponse,
    WorkerAckRequest, WorkerAckResponse, WorkerExtendLeaseRequest, WorkerHeartbeatRequest,
    WorkerLeaseResponse, WorkerPollRequest, WorkerPollResponse, WorkerReportStepRequest,
};
use super::lease::{LeaseConfig, LeaseManager, RepositoryLeaseManager};
use super::models::AttemptExecutionStatus;
use super::repository::RuntimeRepository;
#[cfg(feature = "sqlite-persistence")]
use super::sqlite_runtime_repository::{
    AttemptTraceContextRow, AuditLogEntry, DeadLetterRow, ReplayEffectClaim, RetryPolicyConfig,
    RetryStrategy, SqliteRuntimeRepository, StepReportWriteResult, TimeoutPolicyConfig,
};
use tracing::{info_span, Instrument};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ApiRole {
    Admin,
    Operator,
    Worker,
}

impl ApiRole {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Admin => "admin",
            Self::Operator => "operator",
            Self::Worker => "worker",
        }
    }

    fn from_str(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "admin" => Some(Self::Admin),
            "operator" => Some(Self::Operator),
            "worker" => Some(Self::Worker),
            _ => None,
        }
    }
}

impl Default for ApiRole {
    fn default() -> Self {
        Self::Admin
    }
}

#[derive(Clone, Debug)]
struct AuthContext {
    actor_type: String,
    actor_id: Option<String>,
    role: ApiRole,
}

#[derive(Clone, Debug, Default)]
pub struct ExecutionApiAuthConfig {
    pub bearer_token: Option<String>,
    pub bearer_role: ApiRole,
    pub api_key_hash: Option<String>,
    pub api_key_role: ApiRole,
    pub keyed_api_keys: HashMap<String, StaticApiKeyConfig>,
}

#[derive(Clone, Debug)]
pub struct StaticApiKeyConfig {
    pub secret_hash: String,
    pub active: bool,
    pub role: ApiRole,
}

const METRIC_BUCKETS_MS: [f64; 9] = [1.0, 5.0, 10.0, 25.0, 50.0, 100.0, 250.0, 500.0, 1000.0];

#[derive(Clone, Debug)]
pub struct RuntimeMetrics {
    inner: Arc<Mutex<RuntimeMetricsInner>>,
}

#[derive(Debug)]
struct RuntimeMetricsInner {
    lease_operations_total: u64,
    lease_conflicts_total: u64,
    backpressure_worker_limit_total: u64,
    backpressure_tenant_limit_total: u64,
    terminal_acks_completed_total: u64,
    terminal_acks_failed_total: u64,
    terminal_acks_cancelled_total: u64,
    dispatch_latency_ms_sum: f64,
    dispatch_latency_ms_count: u64,
    dispatch_latency_ms_buckets: [u64; METRIC_BUCKETS_MS.len()],
    recovery_latency_ms_sum: f64,
    recovery_latency_ms_count: u64,
    recovery_latency_ms_buckets: [u64; METRIC_BUCKETS_MS.len()],
}

#[derive(Clone, Debug)]
struct RuntimeMetricsSnapshot {
    lease_operations_total: u64,
    lease_conflicts_total: u64,
    backpressure_worker_limit_total: u64,
    backpressure_tenant_limit_total: u64,
    terminal_acks_completed_total: u64,
    terminal_acks_failed_total: u64,
    terminal_acks_cancelled_total: u64,
    dispatch_latency_ms_sum: f64,
    dispatch_latency_ms_count: u64,
    dispatch_latency_ms_buckets: [u64; METRIC_BUCKETS_MS.len()],
    recovery_latency_ms_sum: f64,
    recovery_latency_ms_count: u64,
    recovery_latency_ms_buckets: [u64; METRIC_BUCKETS_MS.len()],
}

impl Default for RuntimeMetrics {
    fn default() -> Self {
        Self {
            inner: Arc::new(Mutex::new(RuntimeMetricsInner {
                lease_operations_total: 0,
                lease_conflicts_total: 0,
                backpressure_worker_limit_total: 0,
                backpressure_tenant_limit_total: 0,
                terminal_acks_completed_total: 0,
                terminal_acks_failed_total: 0,
                terminal_acks_cancelled_total: 0,
                dispatch_latency_ms_sum: 0.0,
                dispatch_latency_ms_count: 0,
                dispatch_latency_ms_buckets: [0; METRIC_BUCKETS_MS.len()],
                recovery_latency_ms_sum: 0.0,
                recovery_latency_ms_count: 0,
                recovery_latency_ms_buckets: [0; METRIC_BUCKETS_MS.len()],
            })),
        }
    }
}

impl RuntimeMetrics {
    fn record_lease_operation(&self) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.lease_operations_total += 1;
        }
    }

    fn record_lease_conflict(&self) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.lease_conflicts_total += 1;
        }
    }

    fn record_backpressure(&self, reason: &str) {
        if let Ok(mut inner) = self.inner.lock() {
            match reason {
                "worker_limit" => inner.backpressure_worker_limit_total += 1,
                "tenant_limit" => inner.backpressure_tenant_limit_total += 1,
                _ => {}
            }
        }
    }

    fn record_terminal_ack(&self, status: &AttemptExecutionStatus) {
        if let Ok(mut inner) = self.inner.lock() {
            match status {
                AttemptExecutionStatus::Completed => inner.terminal_acks_completed_total += 1,
                AttemptExecutionStatus::Failed => inner.terminal_acks_failed_total += 1,
                AttemptExecutionStatus::Cancelled => inner.terminal_acks_cancelled_total += 1,
                _ => {}
            }
        }
    }

    fn record_dispatch_latency_ms(&self, latency_ms: f64) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.dispatch_latency_ms_sum += latency_ms;
            inner.dispatch_latency_ms_count += 1;
            record_histogram_bucket(&mut inner.dispatch_latency_ms_buckets, latency_ms);
        }
    }

    fn record_recovery_latency_ms(&self, latency_ms: f64) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.recovery_latency_ms_sum += latency_ms;
            inner.recovery_latency_ms_count += 1;
            record_histogram_bucket(&mut inner.recovery_latency_ms_buckets, latency_ms);
        }
    }

    fn snapshot(&self) -> RuntimeMetricsSnapshot {
        if let Ok(inner) = self.inner.lock() {
            RuntimeMetricsSnapshot {
                lease_operations_total: inner.lease_operations_total,
                lease_conflicts_total: inner.lease_conflicts_total,
                backpressure_worker_limit_total: inner.backpressure_worker_limit_total,
                backpressure_tenant_limit_total: inner.backpressure_tenant_limit_total,
                terminal_acks_completed_total: inner.terminal_acks_completed_total,
                terminal_acks_failed_total: inner.terminal_acks_failed_total,
                terminal_acks_cancelled_total: inner.terminal_acks_cancelled_total,
                dispatch_latency_ms_sum: inner.dispatch_latency_ms_sum,
                dispatch_latency_ms_count: inner.dispatch_latency_ms_count,
                dispatch_latency_ms_buckets: inner.dispatch_latency_ms_buckets,
                recovery_latency_ms_sum: inner.recovery_latency_ms_sum,
                recovery_latency_ms_count: inner.recovery_latency_ms_count,
                recovery_latency_ms_buckets: inner.recovery_latency_ms_buckets,
            }
        } else {
            RuntimeMetricsSnapshot {
                lease_operations_total: 0,
                lease_conflicts_total: 0,
                backpressure_worker_limit_total: 0,
                backpressure_tenant_limit_total: 0,
                terminal_acks_completed_total: 0,
                terminal_acks_failed_total: 0,
                terminal_acks_cancelled_total: 0,
                dispatch_latency_ms_sum: 0.0,
                dispatch_latency_ms_count: 0,
                dispatch_latency_ms_buckets: [0; METRIC_BUCKETS_MS.len()],
                recovery_latency_ms_sum: 0.0,
                recovery_latency_ms_count: 0,
                recovery_latency_ms_buckets: [0; METRIC_BUCKETS_MS.len()],
            }
        }
    }

    fn render_prometheus(&self, queue_depth: usize) -> String {
        let snapshot = self.snapshot();
        let conflict_rate = if snapshot.lease_operations_total == 0 {
            0.0
        } else {
            snapshot.lease_conflicts_total as f64 / snapshot.lease_operations_total as f64
        };
        let terminal_acks_total = snapshot.terminal_acks_completed_total
            + snapshot.terminal_acks_failed_total
            + snapshot.terminal_acks_cancelled_total;
        let terminal_error_rate = if terminal_acks_total == 0 {
            0.0
        } else {
            (snapshot.terminal_acks_failed_total + snapshot.terminal_acks_cancelled_total) as f64
                / terminal_acks_total as f64
        };

        let mut out = String::new();
        out.push_str(
            "# HELP oris_runtime_queue_depth Number of dispatchable attempts currently queued.\n",
        );
        out.push_str("# TYPE oris_runtime_queue_depth gauge\n");
        out.push_str(&format!("oris_runtime_queue_depth {}\n", queue_depth));
        out.push_str("# HELP oris_runtime_lease_operations_total Total lease-sensitive operations observed.\n");
        out.push_str("# TYPE oris_runtime_lease_operations_total counter\n");
        out.push_str(&format!(
            "oris_runtime_lease_operations_total {}\n",
            snapshot.lease_operations_total
        ));
        out.push_str("# HELP oris_runtime_lease_conflicts_total Total lease conflicts observed.\n");
        out.push_str("# TYPE oris_runtime_lease_conflicts_total counter\n");
        out.push_str(&format!(
            "oris_runtime_lease_conflicts_total {}\n",
            snapshot.lease_conflicts_total
        ));
        out.push_str("# HELP oris_runtime_lease_conflict_rate Lease conflicts divided by lease operations.\n");
        out.push_str("# TYPE oris_runtime_lease_conflict_rate gauge\n");
        out.push_str(&format!(
            "oris_runtime_lease_conflict_rate {:.6}\n",
            conflict_rate
        ));
        out.push_str("# HELP oris_runtime_backpressure_total Total worker poll backpressure decisions by reason.\n");
        out.push_str("# TYPE oris_runtime_backpressure_total counter\n");
        out.push_str(&format!(
            "oris_runtime_backpressure_total{{reason=\"worker_limit\"}} {}\n",
            snapshot.backpressure_worker_limit_total
        ));
        out.push_str(&format!(
            "oris_runtime_backpressure_total{{reason=\"tenant_limit\"}} {}\n",
            snapshot.backpressure_tenant_limit_total
        ));
        out.push_str("# HELP oris_runtime_terminal_acks_total Total terminal worker acknowledgements by status.\n");
        out.push_str("# TYPE oris_runtime_terminal_acks_total counter\n");
        out.push_str(&format!(
            "oris_runtime_terminal_acks_total{{status=\"completed\"}} {}\n",
            snapshot.terminal_acks_completed_total
        ));
        out.push_str(&format!(
            "oris_runtime_terminal_acks_total{{status=\"failed\"}} {}\n",
            snapshot.terminal_acks_failed_total
        ));
        out.push_str(&format!(
            "oris_runtime_terminal_acks_total{{status=\"cancelled\"}} {}\n",
            snapshot.terminal_acks_cancelled_total
        ));
        out.push_str("# HELP oris_runtime_terminal_error_rate Terminal failed/cancelled acknowledgements divided by all terminal acknowledgements.\n");
        out.push_str("# TYPE oris_runtime_terminal_error_rate gauge\n");
        out.push_str(&format!(
            "oris_runtime_terminal_error_rate {:.6}\n",
            terminal_error_rate
        ));
        render_histogram(
            &mut out,
            "oris_runtime_dispatch_latency_ms",
            "Dispatch latency in milliseconds.",
            &snapshot.dispatch_latency_ms_buckets,
            snapshot.dispatch_latency_ms_count,
            snapshot.dispatch_latency_ms_sum,
        );
        render_histogram(
            &mut out,
            "oris_runtime_recovery_latency_ms",
            "Failover recovery latency in milliseconds.",
            &snapshot.recovery_latency_ms_buckets,
            snapshot.recovery_latency_ms_count,
            snapshot.recovery_latency_ms_sum,
        );
        out
    }
}

fn record_histogram_bucket(buckets: &mut [u64; METRIC_BUCKETS_MS.len()], value: f64) {
    for (idx, upper_bound) in METRIC_BUCKETS_MS.iter().enumerate() {
        if value <= *upper_bound {
            buckets[idx] += 1;
        }
    }
}

fn render_histogram(
    out: &mut String,
    metric_name: &str,
    help: &str,
    buckets: &[u64; METRIC_BUCKETS_MS.len()],
    count: u64,
    sum: f64,
) {
    out.push_str(&format!("# HELP {} {}\n", metric_name, help));
    out.push_str(&format!("# TYPE {} histogram\n", metric_name));
    for (idx, upper_bound) in METRIC_BUCKETS_MS.iter().enumerate() {
        out.push_str(&format!(
            "{}_bucket{{le=\"{}\"}} {}\n",
            metric_name, upper_bound, buckets[idx]
        ));
    }
    out.push_str(&format!(
        "{}_bucket{{le=\"+Inf\"}} {}\n",
        metric_name, count
    ));
    out.push_str(&format!("{}_sum {:.6}\n", metric_name, sum));
    out.push_str(&format!("{}_count {}\n", metric_name, count));
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
            bearer_role: ApiRole::Admin,
            api_key_hash: Self::normalize_hashed_secret(api_key),
            api_key_role: ApiRole::Admin,
            keyed_api_keys: HashMap::new(),
        }
    }

    fn set_keyed_api_key(&mut self, key_id: String, secret: String, active: bool, role: ApiRole) {
        if let (Some(key_id), Some(secret)) = (
            Self::normalize_key_id(Some(key_id)),
            Self::normalize_secret(Some(secret)),
        ) {
            self.keyed_api_keys.insert(
                key_id,
                StaticApiKeyConfig {
                    secret_hash: Self::secret_hash(secret.as_str()),
                    active,
                    role,
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
    pub runtime_metrics: RuntimeMetrics,
    pub worker_poll_limit: usize,
    pub max_active_leases_per_worker: usize,
    pub max_active_leases_per_tenant: usize,
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
            runtime_metrics: RuntimeMetrics::default(),
            worker_poll_limit: 1,
            max_active_leases_per_worker: 8,
            max_active_leases_per_tenant: 8,
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

    pub fn with_static_auth_roles(mut self, bearer_role: ApiRole, api_key_role: ApiRole) -> Self {
        self.auth.bearer_role = bearer_role;
        self.auth.api_key_role = api_key_role;
        self
    }

    pub fn with_static_bearer_token(mut self, token: impl Into<String>) -> Self {
        self.auth.bearer_token = ExecutionApiAuthConfig::normalize_secret(Some(token.into()));
        self.auth.bearer_role = ApiRole::Admin;
        self
    }

    pub fn with_static_bearer_token_with_role(
        mut self,
        token: impl Into<String>,
        role: ApiRole,
    ) -> Self {
        self.auth.bearer_token = ExecutionApiAuthConfig::normalize_secret(Some(token.into()));
        self.auth.bearer_role = role;
        self
    }

    pub fn with_static_api_key(mut self, key: impl Into<String>) -> Self {
        self.auth.api_key_hash = ExecutionApiAuthConfig::normalize_hashed_secret(Some(key.into()));
        self.auth.api_key_role = ApiRole::Admin;
        self
    }

    pub fn with_static_api_key_with_role(mut self, key: impl Into<String>, role: ApiRole) -> Self {
        self.auth.api_key_hash = ExecutionApiAuthConfig::normalize_hashed_secret(Some(key.into()));
        self.auth.api_key_role = role;
        self
    }

    pub fn with_static_api_key_record(
        mut self,
        key_id: impl Into<String>,
        secret: impl Into<String>,
        active: bool,
    ) -> Self {
        self.auth
            .set_keyed_api_key(key_id.into(), secret.into(), active, ApiRole::Operator);
        self
    }

    pub fn with_static_api_key_record_with_role(
        mut self,
        key_id: impl Into<String>,
        secret: impl Into<String>,
        active: bool,
        role: ApiRole,
    ) -> Self {
        self.auth
            .set_keyed_api_key(key_id.into(), secret.into(), active, role);
        self
    }

    #[cfg(feature = "sqlite-persistence")]
    pub fn with_persisted_api_key_record(
        self,
        key_id: impl Into<String>,
        secret: impl Into<String>,
        active: bool,
    ) -> Self {
        self.with_persisted_api_key_record_with_role(key_id, secret, active, ApiRole::Operator)
    }

    #[cfg(feature = "sqlite-persistence")]
    pub fn with_persisted_api_key_record_with_role(
        self,
        key_id: impl Into<String>,
        secret: impl Into<String>,
        active: bool,
        role: ApiRole,
    ) -> Self {
        let key_id = key_id.into();
        let secret = secret.into();
        if let Some(repo) = self.runtime_repo.as_ref() {
            let _ = repo.upsert_api_key_record(
                &key_id,
                &ExecutionApiAuthConfig::secret_hash(secret.as_str()),
                active,
                role.as_str(),
            );
        }
        self
    }
}

pub fn build_router(state: ExecutionApiState) -> Router {
    let secured = Router::new()
        .route("/v1/audit/logs", get(list_audit_logs))
        .route(
            "/v1/attempts/:attempt_id/retries",
            get(list_attempt_retries),
        )
        .route("/v1/dlq", get(list_dead_letters))
        .route("/v1/dlq/:attempt_id", get(get_dead_letter))
        .route("/v1/dlq/:attempt_id/replay", post(replay_dead_letter))
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
        .layer(from_fn_with_state(state.clone(), audit_middleware))
        .with_state(state.clone());

    Router::new()
        .route("/metrics", get(metrics_endpoint))
        .with_state(state)
        .merge(secured)
}

async fn metrics_endpoint(State(state): State<ExecutionApiState>) -> impl IntoResponse {
    #[cfg(feature = "sqlite-persistence")]
    let queue_depth = state
        .runtime_repo
        .as_ref()
        .and_then(|repo| repo.queue_depth(Utc::now()).ok())
        .unwrap_or(0);
    #[cfg(not(feature = "sqlite-persistence"))]
    let queue_depth = 0usize;

    (
        [(CONTENT_TYPE, "text/plain; version=0.0.4; charset=utf-8")],
        state.runtime_metrics.render_prometheus(queue_depth),
    )
}

fn request_id(headers: &HeaderMap) -> String {
    headers
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .and_then(normalize_request_id)
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string())
}

fn normalize_request_id(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.len() > 128 {
        return None;
    }
    if !trimmed
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b':'))
    {
        return None;
    }
    Some(trimmed.to_string())
}

const TRACE_HEADER_NAME: &str = "traceparent";
const TRACE_VERSION: &str = "00";
const DEFAULT_TRACE_FLAGS: &str = "01";

#[derive(Clone, Debug, PartialEq, Eq)]
struct TraceContextState {
    trace_id: String,
    parent_span_id: Option<String>,
    span_id: String,
    trace_flags: String,
}

impl TraceContextState {
    fn new_from_headers(headers: &HeaderMap, rid: &str) -> Result<Self, ApiError> {
        let (trace_id, parent_span_id, trace_flags) = match parse_traceparent_header(headers, rid)?
        {
            Some((trace_id, parent_span_id, trace_flags)) => {
                (trace_id, Some(parent_span_id), trace_flags)
            }
            None => (generate_trace_id(), None, DEFAULT_TRACE_FLAGS.to_string()),
        };
        Ok(Self {
            trace_id,
            parent_span_id,
            span_id: generate_span_id(),
            trace_flags,
        })
    }

    #[cfg(feature = "sqlite-persistence")]
    fn from_row(row: AttemptTraceContextRow) -> Self {
        Self {
            trace_id: row.trace_id,
            parent_span_id: row.parent_span_id,
            span_id: row.span_id,
            trace_flags: row.trace_flags,
        }
    }

    fn to_response(&self) -> TraceContextResponse {
        TraceContextResponse {
            trace_id: self.trace_id.clone(),
            span_id: self.span_id.clone(),
            parent_span_id: self.parent_span_id.clone(),
            traceparent: format_traceparent(&self.trace_id, &self.span_id, &self.trace_flags),
        }
    }
}

fn generate_trace_id() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

fn generate_span_id() -> String {
    let raw = uuid::Uuid::new_v4().simple().to_string();
    raw[..16].to_string()
}

fn format_traceparent(trace_id: &str, span_id: &str, trace_flags: &str) -> String {
    format!("{TRACE_VERSION}-{trace_id}-{span_id}-{trace_flags}")
}

fn parse_traceparent_header(
    headers: &HeaderMap,
    rid: &str,
) -> Result<Option<(String, String, String)>, ApiError> {
    let Some(raw) = headers.get(TRACE_HEADER_NAME) else {
        return Ok(None);
    };
    let raw = raw.to_str().map_err(|_| {
        ApiError::bad_request("traceparent header must be valid ASCII")
            .with_request_id(rid.to_string())
    })?;
    let parts: Vec<_> = raw.trim().split('-').collect();
    if parts.len() != 4
        || parts[0] != TRACE_VERSION
        || !is_hex_id(parts[1], 32)
        || !is_hex_id(parts[2], 16)
        || !is_hex_id(parts[3], 2)
        || parts[1].chars().all(|c| c == '0')
        || parts[2].chars().all(|c| c == '0')
    {
        return Err(ApiError::bad_request(
            "traceparent must use format 00-<32 hex trace_id>-<16 hex span_id>-<2 hex flags>",
        )
        .with_request_id(rid.to_string()));
    }
    Ok(Some((
        parts[1].to_ascii_lowercase(),
        parts[2].to_ascii_lowercase(),
        parts[3].to_ascii_lowercase(),
    )))
}

fn is_hex_id(value: &str, expected_len: usize) -> bool {
    value.len() == expected_len && value.bytes().all(|b| b.is_ascii_hexdigit())
}

fn lifecycle_span(
    operation: &str,
    rid: &str,
    thread_id: Option<&str>,
    attempt_id: Option<&str>,
    worker_id: Option<&str>,
    trace: Option<&TraceContextState>,
) -> tracing::Span {
    let trace_id = trace.map(|ctx| ctx.trace_id.as_str()).unwrap_or("");
    let span_id = trace.map(|ctx| ctx.span_id.as_str()).unwrap_or("");
    let parent_span_id = trace
        .and_then(|ctx| ctx.parent_span_id.as_deref())
        .unwrap_or("");
    info_span!(
        "execution_lifecycle",
        operation = %operation,
        request_id = %rid,
        trace_id = %trace_id,
        span_id = %span_id,
        parent_span_id = %parent_span_id,
        thread_id = %thread_id.unwrap_or(""),
        attempt_id = %attempt_id.unwrap_or(""),
        worker_id = %worker_id.unwrap_or(""),
    )
}

fn payload_hash(
    thread_id: &str,
    input: &str,
    timeout_policy: Option<&TimeoutPolicyRequest>,
    priority: i32,
    tenant_id: Option<&str>,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(thread_id.as_bytes());
    hasher.update(b"|");
    hasher.update(input.as_bytes());
    hasher.update(b"|");
    if let Some(timeout_policy) = timeout_policy {
        if let Ok(bytes) = serde_json::to_vec(timeout_policy) {
            hasher.update(bytes);
        }
    }
    hasher.update(b"|");
    hasher.update(priority.to_string().as_bytes());
    hasher.update(b"|");
    hasher.update(tenant_id.unwrap_or("").as_bytes());
    format!("{:x}", hasher.finalize())
}

fn json_hash(value: &Value) -> Result<String, ApiError> {
    let json = serde_json::to_vec(value)
        .map_err(|e| ApiError::internal(format!("serialize json: {}", e)))?;
    let mut hasher = Sha256::new();
    hasher.update(&json);
    Ok(format!("{:x}", hasher.finalize()))
}

fn replay_effect_fingerprint(thread_id: &str, replay_target: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(thread_id.as_bytes());
    hasher.update(b"|");
    hasher.update(replay_target.as_bytes());
    hasher.update(b"|job_replay_effect");
    format!("{:x}", hasher.finalize())
}

async fn resolve_replay_guard_target(
    state: &ExecutionApiState,
    thread_id: &str,
    requested_checkpoint_id: Option<&str>,
) -> Result<Option<String>, ApiError> {
    if let Some(checkpoint_id) = requested_checkpoint_id {
        return Ok(Some(format!("checkpoint:{}", checkpoint_id)));
    }
    let config = RunnableConfig::with_thread_id(thread_id);
    let snapshot = match state.compiled.get_state(&config).await {
        Ok(snapshot) => snapshot,
        Err(_) => return Ok(None),
    };
    let values = serde_json::to_vec(&snapshot.values)
        .map_err(|e| ApiError::internal(format!("serialize replay target state failed: {}", e)))?;
    let mut hasher = Sha256::new();
    hasher.update(values);
    Ok(Some(format!("latest_state:{:x}", hasher.finalize())))
}

#[cfg(feature = "sqlite-persistence")]
fn map_dead_letter_item(row: DeadLetterRow) -> DeadLetterItem {
    DeadLetterItem {
        attempt_id: row.attempt_id,
        run_id: row.run_id,
        attempt_no: row.attempt_no,
        terminal_status: row.terminal_status,
        reason: row.reason,
        dead_at: row.dead_at.to_rfc3339(),
        replay_status: row.replay_status,
        replay_count: row.replay_count,
        last_replayed_at: row.last_replayed_at.map(|value| value.to_rfc3339()),
    }
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

fn authenticate_static(headers: &HeaderMap, auth: &ExecutionApiAuthConfig) -> Option<AuthContext> {
    if auth
        .bearer_token
        .as_deref()
        .zip(bearer_token_from_headers(headers))
        .map(|(expected, actual)| expected == actual)
        .unwrap_or(false)
    {
        return Some(AuthContext {
            actor_type: "bearer".to_string(),
            actor_id: None,
            role: auth.bearer_role.clone(),
        });
    }

    if auth
        .api_key_hash
        .as_deref()
        .zip(api_key_from_headers(headers))
        .map(|(expected_hash, actual)| expected_hash == ExecutionApiAuthConfig::secret_hash(actual))
        .unwrap_or(false)
    {
        return Some(AuthContext {
            actor_type: "api_key".to_string(),
            actor_id: None,
            role: auth.api_key_role.clone(),
        });
    }

    api_key_id_from_headers(headers)
        .zip(api_key_from_headers(headers))
        .and_then(|(key_id, secret)| {
            auth.keyed_api_keys.get(key_id).and_then(|config| {
                if config.active
                    && config.secret_hash == ExecutionApiAuthConfig::secret_hash(secret)
                {
                    Some(AuthContext {
                        actor_type: "api_key".to_string(),
                        actor_id: Some(key_id.to_string()),
                        role: config.role.clone(),
                    })
                } else {
                    None
                }
            })
        })
}

#[cfg(feature = "sqlite-persistence")]
fn authenticate_runtime_repo(
    headers: &HeaderMap,
    state: &ExecutionApiState,
) -> Option<AuthContext> {
    let Some(repo) = state.runtime_repo.as_ref() else {
        return None;
    };
    let Some(key_id) = api_key_id_from_headers(headers) else {
        return None;
    };
    let Some(secret) = api_key_from_headers(headers) else {
        return None;
    };
    match repo.get_api_key_record(key_id) {
        Ok(Some(record))
            if record.active
                && record.secret_hash == ExecutionApiAuthConfig::secret_hash(secret) =>
        {
            Some(AuthContext {
                actor_type: "api_key".to_string(),
                actor_id: Some(record.key_id),
                role: ApiRole::from_str(&record.role).unwrap_or(ApiRole::Operator),
            })
        }
        _ => None,
    }
}

#[cfg(not(feature = "sqlite-persistence"))]
fn authenticate_runtime_repo(
    _headers: &HeaderMap,
    _state: &ExecutionApiState,
) -> Option<AuthContext> {
    None
}

#[cfg(feature = "sqlite-persistence")]
fn has_runtime_repo_api_keys(state: &ExecutionApiState) -> bool {
    state
        .runtime_repo
        .as_ref()
        .and_then(|repo| repo.has_any_api_keys().ok())
        .unwrap_or(false)
}

#[cfg(not(feature = "sqlite-persistence"))]
fn has_runtime_repo_api_keys(_state: &ExecutionApiState) -> bool {
    false
}

fn resolve_auth_context(headers: &HeaderMap, state: &ExecutionApiState) -> Option<AuthContext> {
    authenticate_static(headers, &state.auth).or_else(|| authenticate_runtime_repo(headers, state))
}

#[derive(Clone, Debug)]
struct AuditTarget {
    action: &'static str,
    resource_type: &'static str,
    resource_id: Option<String>,
}

fn parse_audit_target(method: &axum::http::Method, path: &str) -> Option<AuditTarget> {
    if *method != axum::http::Method::POST {
        return None;
    }
    let seg = path
        .split('/')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>();
    if seg.len() < 2 || seg[0] != "v1" {
        return None;
    }
    match (seg[1], seg.as_slice()) {
        ("jobs", ["v1", "jobs"]) => Some(AuditTarget {
            action: "job.run",
            resource_type: "thread",
            resource_id: None,
        }),
        ("jobs", ["v1", "jobs", "run"]) => Some(AuditTarget {
            action: "job.run",
            resource_type: "thread",
            resource_id: None,
        }),
        ("jobs", ["v1", "jobs", thread_id, "resume"]) => Some(AuditTarget {
            action: "job.resume",
            resource_type: "thread",
            resource_id: Some((*thread_id).to_string()),
        }),
        ("jobs", ["v1", "jobs", thread_id, "replay"]) => Some(AuditTarget {
            action: "job.replay",
            resource_type: "thread",
            resource_id: Some((*thread_id).to_string()),
        }),
        ("jobs", ["v1", "jobs", thread_id, "cancel"]) => Some(AuditTarget {
            action: "job.cancel",
            resource_type: "thread",
            resource_id: Some((*thread_id).to_string()),
        }),
        ("interrupts", ["v1", "interrupts", interrupt_id, "resume"]) => Some(AuditTarget {
            action: "interrupt.resume",
            resource_type: "interrupt",
            resource_id: Some((*interrupt_id).to_string()),
        }),
        ("interrupts", ["v1", "interrupts", interrupt_id, "reject"]) => Some(AuditTarget {
            action: "interrupt.reject",
            resource_type: "interrupt",
            resource_id: Some((*interrupt_id).to_string()),
        }),
        ("dlq", ["v1", "dlq", attempt_id, "replay"]) => Some(AuditTarget {
            action: "dlq.replay",
            resource_type: "attempt",
            resource_id: Some((*attempt_id).to_string()),
        }),
        _ => None,
    }
}

#[cfg(feature = "sqlite-persistence")]
fn append_audit_log(
    state: &ExecutionApiState,
    auth: Option<&AuthContext>,
    target: &AuditTarget,
    request_id: &str,
    method: &str,
    path: &str,
    status_code: u16,
) {
    let Some(repo) = state.runtime_repo.as_ref() else {
        return;
    };
    let entry = AuditLogEntry {
        actor_type: auth
            .map(|a| a.actor_type.clone())
            .unwrap_or_else(|| "anonymous".to_string()),
        actor_id: auth.and_then(|a| a.actor_id.clone()),
        actor_role: auth.map(|a| a.role.as_str().to_string()),
        action: target.action.to_string(),
        resource_type: target.resource_type.to_string(),
        resource_id: target.resource_id.clone(),
        result: if (200..300).contains(&status_code) {
            "success".to_string()
        } else {
            "error".to_string()
        },
        request_id: request_id.to_string(),
        details_json: serde_json::to_string(&serde_json::json!({
            "method": method,
            "path": path,
            "status_code": status_code
        }))
        .ok(),
    };
    let _ = repo.append_audit_log(&entry);
}

#[cfg(not(feature = "sqlite-persistence"))]
fn append_audit_log(
    _state: &ExecutionApiState,
    _auth: Option<&AuthContext>,
    _target: &AuditTarget,
    _request_id: &str,
    _method: &str,
    _path: &str,
    _status_code: u16,
) {
}

async fn audit_middleware(
    State(state): State<ExecutionApiState>,
    headers: HeaderMap,
    request: axum::extract::Request,
    next: Next,
) -> axum::response::Response {
    let method = request.method().clone();
    let path = request.uri().path().to_string();
    let request_id = request_id(&headers);
    let target = parse_audit_target(&method, &path);
    let auth = resolve_auth_context(&headers, &state);
    let response = next.run(request).await;
    if let Some(target) = target {
        append_audit_log(
            &state,
            auth.as_ref(),
            &target,
            &request_id,
            method.as_str(),
            &path,
            response.status().as_u16(),
        );
    }
    response
}

fn role_can_access(role: &ApiRole, method: &axum::http::Method, path: &str) -> bool {
    if matches!(role, ApiRole::Admin) {
        return true;
    }
    let is_jobs_or_interrupts = path.starts_with("/v1/jobs") || path.starts_with("/v1/interrupts");
    let is_workers = path.starts_with("/v1/workers");
    let is_audit = path.starts_with("/v1/audit");
    let is_attempts = path.starts_with("/v1/attempts");
    let is_dlq = path.starts_with("/v1/dlq");
    match role {
        ApiRole::Operator => {
            is_jobs_or_interrupts
                || (is_audit && *method == axum::http::Method::GET)
                || (is_attempts && *method == axum::http::Method::GET)
                || is_dlq
        }
        ApiRole::Worker => {
            // Worker role can only call worker control/data-plane endpoints.
            is_workers && *method != axum::http::Method::GET
        }
        ApiRole::Admin => true,
    }
}

fn supported_auth_methods(state: &ExecutionApiState) -> Vec<&'static str> {
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
    if has_runtime_repo_api_keys(state) {
        methods.push("sqlite:x-api-key-id+x-api-key");
    }
    methods
}

async fn auth_middleware(
    State(state): State<ExecutionApiState>,
    headers: HeaderMap,
    request: axum::extract::Request,
    next: Next,
) -> axum::response::Response {
    let auth_enabled = state.auth.is_enabled() || has_runtime_repo_api_keys(&state);
    if !auth_enabled {
        return next.run(request).await;
    }

    let method = request.method().clone();
    let path = request.uri().path().to_string();

    let auth = resolve_auth_context(&headers, &state);
    let Some(auth) = auth else {
        let rid = request_id(&headers);
        return ApiError::unauthorized("missing or invalid credentials")
            .with_request_id(rid)
            .with_details(serde_json::json!({ "supported_auth": supported_auth_methods(&state) }))
            .into_response();
    };

    if !role_can_access(&auth.role, &method, &path) {
        let rid = request_id(&headers);
        return ApiError::forbidden("role is not allowed to access this endpoint")
            .with_request_id(rid)
            .with_details(serde_json::json!({
                "role": auth.role.as_str(),
                "method": method.as_str(),
                "path": path
            }))
            .into_response();
    }

    next.run(request).await
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

#[cfg(feature = "sqlite-persistence")]
fn parse_retry_policy(
    request: Option<&RetryPolicyRequest>,
    rid: &str,
) -> Result<Option<RetryPolicyConfig>, ApiError> {
    let Some(request) = request else {
        return Ok(None);
    };
    if request.backoff_ms <= 0 {
        return Err(ApiError::bad_request("retry_policy.backoff_ms must be > 0")
            .with_request_id(rid.to_string()));
    }
    let strategy = RetryStrategy::from_str(&request.strategy).ok_or_else(|| {
        ApiError::bad_request("retry_policy.strategy must be one of: fixed|exponential")
            .with_request_id(rid.to_string())
    })?;
    let max_backoff_ms = match request.max_backoff_ms {
        Some(value) if value <= 0 => {
            return Err(
                ApiError::bad_request("retry_policy.max_backoff_ms must be > 0")
                    .with_request_id(rid.to_string()),
            )
        }
        Some(value) if value < request.backoff_ms => {
            return Err(ApiError::bad_request(
                "retry_policy.max_backoff_ms must be >= retry_policy.backoff_ms",
            )
            .with_request_id(rid.to_string()))
        }
        value => value,
    };
    let multiplier = match strategy {
        RetryStrategy::Fixed => None,
        RetryStrategy::Exponential => {
            let value = request.multiplier.unwrap_or(2.0);
            if value <= 1.0 {
                return Err(ApiError::bad_request(
                    "retry_policy.multiplier must be > 1.0 for exponential backoff",
                )
                .with_request_id(rid.to_string()));
            }
            Some(value)
        }
    };
    Ok(Some(RetryPolicyConfig {
        strategy,
        backoff_ms: request.backoff_ms,
        max_backoff_ms,
        multiplier,
        max_retries: request.max_retries,
    }))
}

#[cfg(feature = "sqlite-persistence")]
fn parse_timeout_policy(
    request: Option<&TimeoutPolicyRequest>,
    rid: &str,
) -> Result<Option<TimeoutPolicyConfig>, ApiError> {
    let Some(request) = request else {
        return Ok(None);
    };
    if request.timeout_ms <= 0 {
        return Err(
            ApiError::bad_request("timeout_policy.timeout_ms must be > 0")
                .with_request_id(rid.to_string()),
        );
    }
    let on_timeout_status = match request
        .on_timeout_status
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "failed" => AttemptExecutionStatus::Failed,
        "cancelled" => AttemptExecutionStatus::Cancelled,
        _ => {
            return Err(ApiError::bad_request(
                "timeout_policy.on_timeout_status must be one of: failed|cancelled",
            )
            .with_request_id(rid.to_string()))
        }
    };
    Ok(Some(TimeoutPolicyConfig {
        timeout_ms: request.timeout_ms,
        on_timeout_status,
    }))
}

fn parse_priority(priority: Option<i32>, rid: &str) -> Result<i32, ApiError> {
    let priority = priority.unwrap_or(0);
    if !(0..=100).contains(&priority) {
        return Err(ApiError::bad_request("priority must be between 0 and 100")
            .with_request_id(rid.to_string()));
    }
    Ok(priority)
}

fn parse_tenant_id(tenant_id: Option<&str>, rid: &str) -> Result<Option<String>, ApiError> {
    let Some(tenant_id) = tenant_id.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    if tenant_id.len() > 128 {
        return Err(
            ApiError::bad_request("tenant_id must be 128 characters or fewer")
                .with_request_id(rid.to_string()),
        );
    }
    if !tenant_id
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b':'))
    {
        return Err(
            ApiError::bad_request("tenant_id contains unsupported characters")
                .with_request_id(rid.to_string()),
        );
    }
    Ok(Some(tenant_id.to_string()))
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
    let priority = parse_priority(req.priority, &rid)?;
    let tenant_id = parse_tenant_id(req.tenant_id.as_deref(), &rid)?;
    let request_payload_hash = payload_hash(
        &req.thread_id,
        &input,
        req.timeout_policy.as_ref(),
        priority,
        tenant_id.as_deref(),
    );
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

    let run_trace = TraceContextState::new_from_headers(&headers, &rid)?;
    let run_span = lifecycle_span(
        "job.run",
        &rid,
        Some(&req.thread_id),
        None,
        None,
        Some(&run_trace),
    );

    let initial = MessagesState::with_messages(vec![Message::new_human_message(input)]);
    let config = RunnableConfig::with_thread_id(&req.thread_id);
    let result = state
        .compiled
        .invoke_with_config_interrupt(StateOrCommand::State(initial), &config)
        .instrument(run_span)
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
        trace: Some(run_trace.to_response()),
    };

    #[cfg(feature = "sqlite-persistence")]
    let timeout_policy = parse_timeout_policy(req.timeout_policy.as_ref(), &rid)?;

    #[cfg(feature = "sqlite-persistence")]
    {
        if (timeout_policy.is_some() || priority != 0 || tenant_id.is_some())
            && state.runtime_repo.is_none()
        {
            return Err(ApiError::internal(
                "runtime scheduling options require runtime repository",
            )
            .with_request_id(rid.clone()));
        }
    }

    #[cfg(feature = "sqlite-persistence")]
    if let Some(repo) = state.runtime_repo.as_ref() {
        let attempt_id = format!("attempt-{}-{}", req.thread_id, uuid::Uuid::new_v4());
        let _ = repo.upsert_job(&req.thread_id, &status);
        let _ = repo.enqueue_attempt(&attempt_id, &req.thread_id);
        let _ = repo.set_attempt_priority(&attempt_id, priority);
        let _ = repo.set_attempt_tenant_id(&attempt_id, tenant_id.as_deref());
        let _ = repo.set_attempt_trace_context(
            &attempt_id,
            &run_trace.trace_id,
            run_trace.parent_span_id.as_deref(),
            &run_trace.span_id,
            &run_trace.trace_flags,
        );
        if let Some(policy) = timeout_policy.as_ref() {
            let _ = repo.set_attempt_timeout_policy(&attempt_id, policy);
        }
        if !interrupts.is_empty() {
            let interrupt_attempt_id = format!("attempt-{}-main", req.thread_id);
            for (i, iv) in interrupts.iter().enumerate() {
                let interrupt_id = format!("int-{}-{}", req.thread_id, i);
                let value_json = serde_json::to_string(iv).unwrap_or_default();
                let _ = repo.insert_interrupt(
                    &interrupt_id,
                    &req.thread_id,
                    &req.thread_id,
                    &interrupt_attempt_id,
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
            trace: None,
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

    #[cfg(feature = "sqlite-persistence")]
    let replay_guard = if let Some(repo) = state.runtime_repo.as_ref() {
        let replay_target =
            resolve_replay_guard_target(&state, &thread_id, req.checkpoint_id.as_deref())
                .await
                .map_err(|e| e.with_request_id(rid.clone()))?;
        if let Some(replay_target) = replay_target {
            let fingerprint = replay_effect_fingerprint(&thread_id, &replay_target);
            match repo.claim_replay_effect(&thread_id, &replay_target, &fingerprint, Utc::now()) {
                Ok(ReplayEffectClaim::Acquired) => Some(fingerprint),
                Ok(ReplayEffectClaim::InProgress) => {
                    return Err(
                        ApiError::conflict("replay already in progress for this target")
                            .with_request_id(rid.clone()),
                    );
                }
                Ok(ReplayEffectClaim::Completed(response_json)) => {
                    let mut response: RunJobResponse = serde_json::from_str(&response_json)
                        .map_err(|e| {
                            ApiError::internal(format!(
                                "decode stored replay response failed: {}",
                                e
                            ))
                            .with_request_id(rid.clone())
                        })?;
                    response.idempotent_replay = true;
                    return Ok(Json(ApiEnvelope {
                        meta: ApiMeta::ok(),
                        request_id: rid,
                        data: response,
                    }));
                }
                Err(e) => {
                    return Err(
                        ApiError::internal(format!("replay effect guard failed: {}", e))
                            .with_request_id(rid.clone()),
                    );
                }
            }
        } else {
            None
        }
    } else {
        None
    };

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

    let _state = match state.compiled.invoke_with_config(None, &config).await {
        Ok(executed_state) => executed_state,
        Err(e) => {
            #[cfg(feature = "sqlite-persistence")]
            if let (Some(repo), Some(fingerprint)) =
                (state.runtime_repo.as_ref(), replay_guard.as_deref())
            {
                let _ = repo.abandon_replay_effect(fingerprint);
            }
            return Err(
                ApiError::internal(format!("replay failed: {}", e)).with_request_id(rid.clone())
            );
        }
    };

    let response = RunJobResponse {
        thread_id: thread_id.clone(),
        status: "completed".to_string(),
        interrupts: Vec::new(),
        idempotency_key: None,
        idempotent_replay: false,
        trace: None,
    };

    #[cfg(feature = "sqlite-persistence")]
    if let (Some(repo), Some(fingerprint)) = (state.runtime_repo.as_ref(), replay_guard.as_deref())
    {
        let response_json = serde_json::to_string(&response).map_err(|e| {
            ApiError::internal(format!("encode replay response failed: {}", e))
                .with_request_id(rid.clone())
        })?;
        repo.complete_replay_effect(fingerprint, &response_json, Utc::now())
            .map_err(|e| {
                ApiError::internal(format!("persist replay effect failed: {}", e))
                    .with_request_id(rid.clone())
            })?;
    }

    Ok(Json(ApiEnvelope {
        meta: ApiMeta::ok(),
        request_id: rid,
        data: response,
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

pub async fn list_audit_logs(
    State(state): State<ExecutionApiState>,
    headers: HeaderMap,
    Query(q): Query<ListAuditLogsQuery>,
) -> Result<Json<ApiEnvelope<AuditLogListResponse>>, ApiError> {
    let rid = request_id(&headers);
    if let (Some(from_ms), Some(to_ms)) = (q.from_ms, q.to_ms) {
        if from_ms > to_ms {
            return Err(
                ApiError::bad_request("from_ms must be less than or equal to to_ms")
                    .with_request_id(rid),
            );
        }
    }
    #[cfg(feature = "sqlite-persistence")]
    {
        let repo = runtime_repo(&state, &rid)?.clone();
        let limit = q.limit.unwrap_or(100).clamp(1, 500);
        let request_id_filter = q
            .request_id
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty());
        let action_filter = q.action.as_deref().map(str::trim).filter(|v| !v.is_empty());
        let rows = repo
            .list_audit_logs_filtered(request_id_filter, action_filter, q.from_ms, q.to_ms, limit)
            .map_err(|e| ApiError::internal(e.to_string()).with_request_id(rid.clone()))?;
        let logs = rows
            .into_iter()
            .map(|row| AuditLogItem {
                audit_id: row.audit_id,
                actor_type: row.actor_type,
                actor_id: row.actor_id,
                actor_role: row.actor_role,
                action: row.action,
                resource_type: row.resource_type,
                resource_id: row.resource_id,
                result: row.result,
                request_id: row.request_id,
                details: row.details_json.and_then(|raw| {
                    serde_json::from_str::<Value>(&raw)
                        .ok()
                        .or_else(|| Some(Value::String(raw)))
                }),
                created_at: row.created_at.to_rfc3339(),
            })
            .collect();
        return Ok(Json(ApiEnvelope {
            meta: ApiMeta::ok(),
            request_id: rid,
            data: AuditLogListResponse { logs },
        }));
    }
    #[cfg(not(feature = "sqlite-persistence"))]
    {
        let _ = q;
        Err(ApiError::internal("audit log APIs require sqlite-persistence").with_request_id(rid))
    }
}

pub async fn list_attempt_retries(
    State(state): State<ExecutionApiState>,
    Path(attempt_id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<ApiEnvelope<AttemptRetryHistoryResponse>>, ApiError> {
    let rid = request_id(&headers);
    if attempt_id.trim().is_empty() {
        return Err(ApiError::bad_request("attempt_id must not be empty").with_request_id(rid));
    }
    #[cfg(feature = "sqlite-persistence")]
    {
        let repo = runtime_repo(&state, &rid)?.clone();
        let snapshot = repo
            .get_attempt_retry_history(&attempt_id)
            .map_err(|e| ApiError::internal(e.to_string()).with_request_id(rid.clone()))?
            .ok_or_else(|| ApiError::not_found("attempt not found").with_request_id(rid.clone()))?;
        let history = snapshot
            .history
            .into_iter()
            .enumerate()
            .map(|(idx, row)| AttemptRetryHistoryItem {
                retry_no: (idx + 1) as u32,
                attempt_no: row.attempt_no,
                strategy: row.strategy,
                backoff_ms: row.backoff_ms,
                max_retries: row.max_retries,
                scheduled_at: row.scheduled_at.to_rfc3339(),
            })
            .collect();
        return Ok(Json(ApiEnvelope {
            meta: ApiMeta::ok(),
            request_id: rid,
            data: AttemptRetryHistoryResponse {
                attempt_id: snapshot.attempt_id,
                current_attempt_no: snapshot.current_attempt_no,
                current_status: match snapshot.current_status {
                    AttemptExecutionStatus::Queued => "queued".to_string(),
                    AttemptExecutionStatus::Leased => "leased".to_string(),
                    AttemptExecutionStatus::Running => "running".to_string(),
                    AttemptExecutionStatus::RetryBackoff => "retry_backoff".to_string(),
                    AttemptExecutionStatus::Completed => "completed".to_string(),
                    AttemptExecutionStatus::Failed => "failed".to_string(),
                    AttemptExecutionStatus::Cancelled => "cancelled".to_string(),
                },
                history,
            },
        }));
    }
    #[cfg(not(feature = "sqlite-persistence"))]
    {
        let _ = attempt_id;
        Err(
            ApiError::internal("attempt retry APIs require sqlite-persistence")
                .with_request_id(rid),
        )
    }
}

pub async fn list_dead_letters(
    State(state): State<ExecutionApiState>,
    headers: HeaderMap,
    Query(q): Query<ListDeadLettersQuery>,
) -> Result<Json<ApiEnvelope<DeadLetterListResponse>>, ApiError> {
    let rid = request_id(&headers);
    #[cfg(feature = "sqlite-persistence")]
    {
        let repo = runtime_repo(&state, &rid)?.clone();
        let limit = q.limit.unwrap_or(100).clamp(1, 500);
        let status_filter = q.status.as_deref().map(str::trim).filter(|v| !v.is_empty());
        let rows = repo
            .list_dead_letters(status_filter, limit)
            .map_err(|e| ApiError::internal(e.to_string()).with_request_id(rid.clone()))?;
        let entries = rows.into_iter().map(map_dead_letter_item).collect();
        return Ok(Json(ApiEnvelope {
            meta: ApiMeta::ok(),
            request_id: rid,
            data: DeadLetterListResponse { entries },
        }));
    }
    #[cfg(not(feature = "sqlite-persistence"))]
    {
        let _ = q;
        Err(ApiError::internal("dlq APIs require sqlite-persistence").with_request_id(rid))
    }
}

pub async fn get_dead_letter(
    State(state): State<ExecutionApiState>,
    Path(attempt_id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<ApiEnvelope<DeadLetterItem>>, ApiError> {
    let rid = request_id(&headers);
    if attempt_id.trim().is_empty() {
        return Err(ApiError::bad_request("attempt_id must not be empty").with_request_id(rid));
    }
    #[cfg(feature = "sqlite-persistence")]
    {
        let repo = runtime_repo(&state, &rid)?.clone();
        let row = repo
            .get_dead_letter(&attempt_id)
            .map_err(|e| ApiError::internal(e.to_string()).with_request_id(rid.clone()))?
            .ok_or_else(|| {
                ApiError::not_found("dead letter not found").with_request_id(rid.clone())
            })?;
        return Ok(Json(ApiEnvelope {
            meta: ApiMeta::ok(),
            request_id: rid,
            data: map_dead_letter_item(row),
        }));
    }
    #[cfg(not(feature = "sqlite-persistence"))]
    {
        let _ = attempt_id;
        Err(ApiError::internal("dlq APIs require sqlite-persistence").with_request_id(rid))
    }
}

pub async fn replay_dead_letter(
    State(state): State<ExecutionApiState>,
    Path(attempt_id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<ApiEnvelope<DeadLetterReplayResponse>>, ApiError> {
    let rid = request_id(&headers);
    if attempt_id.trim().is_empty() {
        return Err(ApiError::bad_request("attempt_id must not be empty").with_request_id(rid));
    }
    #[cfg(feature = "sqlite-persistence")]
    {
        let repo = runtime_repo(&state, &rid)?.clone();
        let row = repo
            .replay_dead_letter(&attempt_id, Utc::now())
            .map_err(|e| {
                let msg = e.to_string();
                if msg.contains("not found") {
                    ApiError::not_found(msg).with_request_id(rid.clone())
                } else if msg.contains("already replayed") {
                    ApiError::conflict(msg).with_request_id(rid.clone())
                } else {
                    ApiError::internal(msg).with_request_id(rid.clone())
                }
            })?;
        return Ok(Json(ApiEnvelope {
            meta: ApiMeta::ok(),
            request_id: rid,
            data: DeadLetterReplayResponse {
                attempt_id: row.attempt_id,
                status: "requeued".to_string(),
                replay_count: row.replay_count,
            },
        }));
    }
    #[cfg(not(feature = "sqlite-persistence"))]
    {
        let _ = attempt_id;
        Err(ApiError::internal("dlq APIs require sqlite-persistence").with_request_id(rid))
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
    let trace = {
        #[cfg(feature = "sqlite-persistence")]
        {
            state
                .runtime_repo
                .as_ref()
                .and_then(|repo| repo.latest_attempt_trace_for_run(&thread_id).ok().flatten())
                .map(TraceContextState::from_row)
        }
        #[cfg(not(feature = "sqlite-persistence"))]
        {
            None
        }
    };
    let _span = lifecycle_span(
        "job.detail",
        &rid,
        Some(&thread_id),
        None,
        None,
        trace.as_ref(),
    )
    .entered();
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
            trace: trace.map(|ctx| ctx.to_response()),
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
        let max_active_per_tenant = req
            .tenant_max_active_leases
            .unwrap_or(state.max_active_leases_per_tenant)
            .max(1);
        let now = Utc::now();
        let poll_started = Instant::now();

        let lease_manager = RepositoryLeaseManager::new(repo.clone(), LeaseConfig::default());
        lease_manager
            .tick(now)
            .map_err(|e| ApiError::internal(e.to_string()).with_request_id(rid.clone()))?;

        let active = repo
            .active_leases_for_worker(&req.worker_id, now)
            .map_err(|e| ApiError::internal(e.to_string()).with_request_id(rid.clone()))?;
        if active >= max_active {
            state.runtime_metrics.record_backpressure("worker_limit");
            return Ok(Json(ApiEnvelope {
                meta: ApiMeta::ok(),
                request_id: rid,
                data: WorkerPollResponse {
                    decision: "backpressure".to_string(),
                    attempt_id: None,
                    lease_id: None,
                    lease_expires_at: None,
                    reason: Some("worker_limit".to_string()),
                    worker_active_leases: Some(active),
                    worker_limit: Some(max_active),
                    tenant_id: None,
                    tenant_active_leases: None,
                    tenant_limit: None,
                    trace: None,
                },
            }));
        }

        let mut tenant_block: Option<(String, usize)> = None;
        let scan_limit = poll_limit.max(16);
        let candidates = repo
            .list_dispatchable_attempt_contexts(now, scan_limit)
            .map_err(|e| ApiError::internal(e.to_string()).with_request_id(rid.clone()))?;

        for candidate in candidates {
            if let Some(tenant_id) = candidate.tenant_id.as_deref() {
                let tenant_active = repo
                    .active_leases_for_tenant(tenant_id, now)
                    .map_err(|e| ApiError::internal(e.to_string()).with_request_id(rid.clone()))?;
                if tenant_active >= max_active_per_tenant {
                    tenant_block = Some((tenant_id.to_string(), tenant_active));
                    continue;
                }
            }

            let lease_expires_at = now + chrono::Duration::seconds(30);
            state.runtime_metrics.record_lease_operation();
            match repo.upsert_lease(&candidate.attempt_id, &req.worker_id, lease_expires_at) {
                Ok(lease) => {
                    let dispatch_latency_ms = poll_started.elapsed().as_secs_f64() * 1000.0;
                    state
                        .runtime_metrics
                        .record_dispatch_latency_ms(dispatch_latency_ms);
                    if candidate.started_at.is_some() {
                        state
                            .runtime_metrics
                            .record_recovery_latency_ms(dispatch_latency_ms);
                    }
                    let dispatch_trace = repo
                        .advance_attempt_trace(&candidate.attempt_id, &generate_span_id())
                        .map_err(|e| {
                            ApiError::internal(e.to_string()).with_request_id(rid.clone())
                        })?
                        .map(TraceContextState::from_row);
                    let _span = lifecycle_span(
                        "attempt.dispatch",
                        &rid,
                        None,
                        Some(&candidate.attempt_id),
                        Some(&req.worker_id),
                        dispatch_trace.as_ref(),
                    )
                    .entered();
                    return Ok(Json(ApiEnvelope {
                        meta: ApiMeta::ok(),
                        request_id: rid,
                        data: WorkerPollResponse {
                            decision: "dispatched".to_string(),
                            attempt_id: Some(candidate.attempt_id),
                            lease_id: Some(lease.lease_id),
                            lease_expires_at: Some(lease.lease_expires_at.to_rfc3339()),
                            reason: None,
                            worker_active_leases: Some(active),
                            worker_limit: Some(max_active),
                            tenant_id: candidate.tenant_id,
                            tenant_active_leases: None,
                            tenant_limit: None,
                            trace: dispatch_trace.map(|ctx| ctx.to_response()),
                        },
                    }));
                }
                Err(err) => {
                    let msg = err.to_string();
                    if msg.contains("active lease already exists")
                        || msg.contains("not dispatchable")
                    {
                        state.runtime_metrics.record_lease_conflict();
                        continue;
                    }
                    return Err(ApiError::internal(msg).with_request_id(rid.clone()));
                }
            }
        }

        if let Some((tenant_id, tenant_active)) = tenant_block {
            state.runtime_metrics.record_backpressure("tenant_limit");
            return Ok(Json(ApiEnvelope {
                meta: ApiMeta::ok(),
                request_id: rid,
                data: WorkerPollResponse {
                    decision: "backpressure".to_string(),
                    attempt_id: None,
                    lease_id: None,
                    lease_expires_at: None,
                    reason: Some("tenant_limit".to_string()),
                    worker_active_leases: Some(active),
                    worker_limit: Some(max_active),
                    tenant_id: Some(tenant_id),
                    tenant_active_leases: Some(tenant_active),
                    tenant_limit: Some(max_active_per_tenant),
                    trace: None,
                },
            }));
        }

        return Ok(Json(ApiEnvelope {
            meta: ApiMeta::ok(),
            request_id: rid,
            data: WorkerPollResponse {
                decision: "noop".to_string(),
                attempt_id: None,
                lease_id: None,
                lease_expires_at: None,
                reason: None,
                worker_active_leases: Some(active),
                worker_limit: Some(max_active),
                tenant_id: None,
                tenant_active_leases: None,
                tenant_limit: Some(max_active_per_tenant),
                trace: None,
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
        state.runtime_metrics.record_lease_operation();
        let lease = repo
            .get_lease_by_id(&req.lease_id)
            .map_err(|e| ApiError::internal(e.to_string()).with_request_id(rid.clone()))?
            .ok_or_else(|| ApiError::not_found("lease not found").with_request_id(rid.clone()))?;
        if lease.worker_id != worker_id {
            state.runtime_metrics.record_lease_conflict();
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
        if let Err(err) = repo.heartbeat_lease_with_version(
            &req.lease_id,
            &worker_id,
            lease.version,
            now,
            expires,
        ) {
            if err.to_string().contains("lease heartbeat version conflict") {
                state.runtime_metrics.record_lease_conflict();
            }
            return Err(ApiError::internal(err.to_string()).with_request_id(rid.clone()));
        }
        let trace = repo
            .advance_attempt_trace(&lease.attempt_id, &generate_span_id())
            .map_err(|e| ApiError::internal(e.to_string()).with_request_id(rid.clone()))?
            .map(TraceContextState::from_row);
        let _span = lifecycle_span(
            "attempt.heartbeat",
            &rid,
            None,
            Some(&lease.attempt_id),
            Some(&worker_id),
            trace.as_ref(),
        )
        .entered();
        return Ok(Json(ApiEnvelope {
            meta: ApiMeta::ok(),
            request_id: rid,
            data: WorkerLeaseResponse {
                worker_id,
                lease_id: req.lease_id,
                lease_expires_at: expires.to_rfc3339(),
                trace: trace.map(|ctx| ctx.to_response()),
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
    let (report_status, report_trace) = {
        let repo = runtime_repo(&state, &rid)?;
        match repo.record_step_report(
            &worker_id,
            &req.attempt_id,
            &req.action_id,
            &req.status,
            &req.dedupe_token,
        ) {
            Ok(outcome) => {
                let trace = repo
                    .advance_attempt_trace(&req.attempt_id, &generate_span_id())
                    .map_err(|e| ApiError::internal(e.to_string()).with_request_id(rid.clone()))?
                    .map(TraceContextState::from_row);
                let status = match outcome {
                    StepReportWriteResult::Inserted => "reported".to_string(),
                    StepReportWriteResult::Duplicate => "reported_idempotent".to_string(),
                };
                (status, trace)
            }
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
    let (report_status, report_trace) = {
        let _ = state;
        ("reported".to_string(), None)
    };

    let _span = lifecycle_span(
        "attempt.step_report",
        &rid,
        None,
        Some(&req.attempt_id),
        Some(&worker_id),
        report_trace.as_ref(),
    )
    .entered();

    Ok(Json(ApiEnvelope {
        meta: ApiMeta::ok(),
        request_id: rid,
        data: WorkerAckResponse {
            attempt_id: req.attempt_id,
            status: report_status,
            next_retry_at: None,
            next_attempt_no: None,
            trace: report_trace.map(|ctx| ctx.to_response()),
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
        let retry_policy = parse_retry_policy(req.retry_policy.as_ref(), &rid)?;
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
        let outcome = repo
            .ack_attempt(&req.attempt_id, status, retry_policy.as_ref(), Utc::now())
            .map_err(|e| ApiError::internal(e.to_string()).with_request_id(rid.clone()))?;
        state.runtime_metrics.record_terminal_ack(&outcome.status);
        let trace = repo
            .advance_attempt_trace(&req.attempt_id, &generate_span_id())
            .map_err(|e| ApiError::internal(e.to_string()).with_request_id(rid.clone()))?
            .map(TraceContextState::from_row);
        let _span = lifecycle_span(
            "attempt.ack",
            &rid,
            None,
            Some(&req.attempt_id),
            Some(&worker_id),
            trace.as_ref(),
        )
        .entered();
        let response_status = match outcome.status {
            AttemptExecutionStatus::RetryBackoff => "retry_scheduled".to_string(),
            AttemptExecutionStatus::Completed => "completed".to_string(),
            AttemptExecutionStatus::Failed => "failed".to_string(),
            AttemptExecutionStatus::Cancelled => "cancelled".to_string(),
            AttemptExecutionStatus::Queued => "queued".to_string(),
            AttemptExecutionStatus::Leased => "leased".to_string(),
            AttemptExecutionStatus::Running => "running".to_string(),
        };
        return Ok(Json(ApiEnvelope {
            meta: ApiMeta::ok(),
            request_id: rid,
            data: WorkerAckResponse {
                attempt_id: req.attempt_id,
                status: response_status,
                next_retry_at: outcome.next_retry_at.map(|value| value.to_rfc3339()),
                next_attempt_no: Some(outcome.next_attempt_no),
                trace: trace.map(|ctx| ctx.to_response()),
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
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::Instant;

    use axum::body::Body;
    use axum::http::{Method, Request, StatusCode};
    use chrono::{Duration, Utc};
    use tower::util::ServiceExt;

    use crate::graph::{
        function_node, interrupt, GraphError, InMemorySaver, MessagesState, StateGraph, END, START,
    };
    use crate::kernel::runtime::models::AttemptExecutionStatus;
    use crate::kernel::runtime::repository::RuntimeRepository;
    #[cfg(feature = "sqlite-persistence")]
    use crate::kernel::runtime::sqlite_runtime_repository::TimeoutPolicyConfig;
    use crate::schemas::messages::Message;

    use super::{build_router, ApiRole, ExecutionApiState};

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

    async fn build_side_effect_graph(
        effect_counter: Arc<AtomicUsize>,
    ) -> Arc<crate::graph::CompiledGraph<MessagesState>> {
        let prepare = function_node("prepare", |_state: &MessagesState| async move {
            Ok(HashMap::new())
        });
        let effect = function_node("effect", move |_state: &MessagesState| {
            let effect_counter = Arc::clone(&effect_counter);
            async move {
                effect_counter.fetch_add(1, Ordering::SeqCst);
                Ok(HashMap::new())
            }
        });
        let wait = function_node("wait", |_state: &MessagesState| async move {
            let _ = interrupt("confirm replay")
                .await
                .map_err(GraphError::InterruptError)?;
            Ok(HashMap::new())
        });
        let mut graph = StateGraph::<MessagesState>::new();
        graph.add_node("prepare", prepare).unwrap();
        graph.add_node("effect", effect).unwrap();
        graph.add_node("wait", wait).unwrap();
        graph.add_edge(START, "prepare");
        graph.add_edge("prepare", "effect");
        graph.add_edge("effect", "wait");
        graph.add_edge("wait", END);
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

    #[cfg(feature = "sqlite-persistence")]
    #[tokio::test]
    async fn auth_sqlite_api_key_record_allows_access() {
        let state =
            ExecutionApiState::with_sqlite_idempotency(build_test_graph().await, ":memory:")
                .with_persisted_api_key_record("db-key-1", "db-secret-1", true);
        let router = build_router(state);
        let req = Request::builder()
            .method(Method::POST)
            .uri("/v1/jobs/run")
            .header("x-api-key-id", "db-key-1")
            .header("x-api-key", "db-secret-1")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "thread_id": "auth-run-7"
                })
                .to_string(),
            ))
            .unwrap();
        let resp = router.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[cfg(feature = "sqlite-persistence")]
    #[tokio::test]
    async fn auth_sqlite_disabled_api_key_is_rejected() {
        let state =
            ExecutionApiState::with_sqlite_idempotency(build_test_graph().await, ":memory:")
                .with_persisted_api_key_record("db-key-2", "db-secret-2", true);
        let repo = state.runtime_repo.clone().expect("runtime repo");
        repo.set_api_key_status("db-key-2", false)
            .expect("disable api key");
        let router = build_router(state);
        let req = Request::builder()
            .method(Method::POST)
            .uri("/v1/jobs/run")
            .header("x-api-key-id", "db-key-2")
            .header("x-api-key", "db-secret-2")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "thread_id": "auth-run-8"
                })
                .to_string(),
            ))
            .unwrap();
        let resp = router.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[cfg(feature = "sqlite-persistence")]
    #[tokio::test]
    async fn auth_sqlite_api_key_table_enforces_auth() {
        let state =
            ExecutionApiState::with_sqlite_idempotency(build_test_graph().await, ":memory:")
                .with_persisted_api_key_record("db-key-3", "db-secret-3", false);
        let router = build_router(state);
        let req = Request::builder()
            .method(Method::POST)
            .uri("/v1/jobs/run")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "thread_id": "auth-run-9"
                })
                .to_string(),
            ))
            .unwrap();
        let resp = router.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn auth_worker_role_cannot_run_jobs() {
        let router = build_router(
            ExecutionApiState::new(build_test_graph().await).with_static_api_key_record_with_role(
                "worker-key-1",
                "worker-secret-1",
                true,
                ApiRole::Worker,
            ),
        );
        let req = Request::builder()
            .method(Method::POST)
            .uri("/v1/jobs/run")
            .header("x-api-key-id", "worker-key-1")
            .header("x-api-key", "worker-secret-1")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "thread_id": "auth-run-10"
                })
                .to_string(),
            ))
            .unwrap();
        let resp = router.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[cfg(feature = "sqlite-persistence")]
    #[tokio::test]
    async fn auth_worker_role_can_access_worker_endpoints() {
        let router = build_router(
            ExecutionApiState::with_sqlite_idempotency(build_test_graph().await, ":memory:")
                .with_static_api_key_record_with_role(
                    "worker-key-2",
                    "worker-secret-2",
                    true,
                    ApiRole::Worker,
                ),
        );
        let req = Request::builder()
            .method(Method::POST)
            .uri("/v1/workers/poll")
            .header("x-api-key-id", "worker-key-2")
            .header("x-api-key", "worker-secret-2")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "worker_id": "worker-rbac-1"
                })
                .to_string(),
            ))
            .unwrap();
        let resp = router.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[cfg(feature = "sqlite-persistence")]
    #[tokio::test]
    async fn auth_operator_role_cannot_access_worker_endpoints() {
        let router = build_router(
            ExecutionApiState::with_sqlite_idempotency(build_test_graph().await, ":memory:")
                .with_static_api_key_record_with_role(
                    "operator-key-1",
                    "operator-secret-1",
                    true,
                    ApiRole::Operator,
                ),
        );
        let req = Request::builder()
            .method(Method::POST)
            .uri("/v1/workers/poll")
            .header("x-api-key-id", "operator-key-1")
            .header("x-api-key", "operator-secret-1")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "worker_id": "worker-rbac-2"
                })
                .to_string(),
            ))
            .unwrap();
        let resp = router.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[cfg(feature = "sqlite-persistence")]
    #[tokio::test]
    async fn auth_admin_role_can_access_worker_endpoints() {
        let router = build_router(
            ExecutionApiState::with_sqlite_idempotency(build_test_graph().await, ":memory:")
                .with_static_api_key_with_role("admin-secret-1", ApiRole::Admin),
        );
        let req = Request::builder()
            .method(Method::POST)
            .uri("/v1/workers/poll")
            .header("x-api-key", "admin-secret-1")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "worker_id": "worker-rbac-3"
                })
                .to_string(),
            ))
            .unwrap();
        let resp = router.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[cfg(feature = "sqlite-persistence")]
    #[tokio::test]
    async fn audit_logs_capture_control_plane_actions() {
        let state =
            ExecutionApiState::with_sqlite_idempotency(build_interrupt_graph().await, ":memory:");
        let repo = state.runtime_repo.clone().expect("runtime repo");
        let router = build_router(state);

        let run_req = Request::builder()
            .method(Method::POST)
            .uri("/v1/jobs/run")
            .header("x-request-id", "req-audit-run")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "thread_id": "audit-job-1",
                    "input": "trigger interrupt"
                })
                .to_string(),
            ))
            .unwrap();
        let run_resp = router.clone().oneshot(run_req).await.unwrap();
        assert_eq!(run_resp.status(), StatusCode::OK);

        let cancel_req = Request::builder()
            .method(Method::POST)
            .uri("/v1/jobs/audit-job-1/cancel")
            .header("x-request-id", "req-audit-cancel")
            .header("content-type", "application/json")
            .body(Body::from("{}"))
            .unwrap();
        let cancel_resp = router.clone().oneshot(cancel_req).await.unwrap();
        assert_eq!(cancel_resp.status(), StatusCode::OK);

        let logs = repo.list_audit_logs(20).expect("list audit logs");
        assert!(logs.iter().any(|l| l.action == "job.run"
            && l.result == "success"
            && l.request_id == "req-audit-run"));
        assert!(logs.iter().any(|l| l.action == "job.cancel"
            && l.result == "success"
            && l.request_id == "req-audit-cancel"));
    }

    #[cfg(feature = "sqlite-persistence")]
    #[tokio::test]
    async fn audit_logs_capture_forbidden_attempts() {
        let state =
            ExecutionApiState::with_sqlite_idempotency(build_test_graph().await, ":memory:")
                .with_static_api_key_record_with_role(
                    "worker-key-audit",
                    "worker-secret-audit",
                    true,
                    ApiRole::Worker,
                );
        let repo = state.runtime_repo.clone().expect("runtime repo");
        let router = build_router(state);

        let req = Request::builder()
            .method(Method::POST)
            .uri("/v1/jobs/run")
            .header("x-request-id", "req-audit-forbidden")
            .header("x-api-key-id", "worker-key-audit")
            .header("x-api-key", "worker-secret-audit")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "thread_id": "audit-job-2"
                })
                .to_string(),
            ))
            .unwrap();
        let resp = router.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);

        let logs = repo.list_audit_logs(10).expect("list audit logs");
        let forbidden = logs
            .iter()
            .find(|l| l.request_id == "req-audit-forbidden")
            .expect("forbidden log");
        assert_eq!(forbidden.action, "job.run");
        assert_eq!(forbidden.result, "error");
        assert_eq!(forbidden.actor_role.as_deref(), Some("worker"));
    }

    #[cfg(feature = "sqlite-persistence")]
    #[tokio::test]
    async fn audit_logs_api_returns_filtered_records_for_operator() {
        let state =
            ExecutionApiState::with_sqlite_idempotency(build_interrupt_graph().await, ":memory:")
                .with_static_api_key_record_with_role(
                    "operator-key-audit-read",
                    "operator-secret-audit-read",
                    true,
                    ApiRole::Operator,
                );
        let router = build_router(state);

        let run_req = Request::builder()
            .method(Method::POST)
            .uri("/v1/jobs/run")
            .header("x-request-id", "req-audit-read-run")
            .header("x-api-key-id", "operator-key-audit-read")
            .header("x-api-key", "operator-secret-audit-read")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "thread_id": "audit-read-1",
                    "input": "trigger interrupt"
                })
                .to_string(),
            ))
            .unwrap();
        let run_resp = router.clone().oneshot(run_req).await.unwrap();
        assert_eq!(run_resp.status(), StatusCode::OK);

        let cancel_req = Request::builder()
            .method(Method::POST)
            .uri("/v1/jobs/audit-read-1/cancel")
            .header("x-request-id", "req-audit-read-cancel")
            .header("x-api-key-id", "operator-key-audit-read")
            .header("x-api-key", "operator-secret-audit-read")
            .header("content-type", "application/json")
            .body(Body::from("{}"))
            .unwrap();
        let cancel_resp = router.clone().oneshot(cancel_req).await.unwrap();
        assert_eq!(cancel_resp.status(), StatusCode::OK);

        let list_req = Request::builder()
            .method(Method::GET)
            .uri("/v1/audit/logs?action=job.cancel&request_id=req-audit-read-cancel&limit=5")
            .header("x-api-key-id", "operator-key-audit-read")
            .header("x-api-key", "operator-secret-audit-read")
            .body(Body::empty())
            .unwrap();
        let list_resp = router.clone().oneshot(list_req).await.unwrap();
        assert_eq!(list_resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(list_resp.into_body(), usize::MAX)
            .await
            .expect("audit list body");
        let json: serde_json::Value = serde_json::from_slice(&body).expect("audit list json");
        let logs = json["data"]["logs"].as_array().expect("logs array");
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0]["action"], "job.cancel");
        assert_eq!(logs[0]["request_id"], "req-audit-read-cancel");
        assert_eq!(logs[0]["actor_role"], "operator");
    }

    #[cfg(feature = "sqlite-persistence")]
    #[tokio::test]
    async fn audit_logs_api_worker_role_is_forbidden() {
        let state =
            ExecutionApiState::with_sqlite_idempotency(build_test_graph().await, ":memory:")
                .with_static_api_key_record_with_role(
                    "worker-key-audit-read",
                    "worker-secret-audit-read",
                    true,
                    ApiRole::Worker,
                );
        let router = build_router(state);

        let req = Request::builder()
            .method(Method::GET)
            .uri("/v1/audit/logs")
            .header("x-api-key-id", "worker-key-audit-read")
            .header("x-api-key", "worker-secret-audit-read")
            .body(Body::empty())
            .unwrap();
        let resp = router.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[cfg(feature = "sqlite-persistence")]
    #[tokio::test]
    async fn audit_logs_api_rejects_invalid_time_range() {
        let state =
            ExecutionApiState::with_sqlite_idempotency(build_test_graph().await, ":memory:")
                .with_static_api_key_record_with_role(
                    "operator-key-audit-range",
                    "operator-secret-audit-range",
                    true,
                    ApiRole::Operator,
                );
        let router = build_router(state);

        let req = Request::builder()
            .method(Method::GET)
            .uri("/v1/audit/logs?from_ms=10&to_ms=1")
            .header("x-api-key-id", "operator-key-audit-range")
            .header("x-api-key", "operator-secret-audit-range")
            .body(Body::empty())
            .unwrap();
        let resp = router.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn security_auth_bypass_with_mixed_invalid_credentials_is_rejected() {
        let router = build_router(
            ExecutionApiState::new(build_test_graph().await).with_static_api_key_record_with_role(
                "sec-key-1",
                "sec-secret-1",
                true,
                ApiRole::Operator,
            ),
        );
        let req = Request::builder()
            .method(Method::POST)
            .uri("/v1/jobs/run")
            .header("authorization", "Bearer definitely-wrong")
            .header("x-api-key-id", "sec-key-1")
            .header("x-api-key", "wrong-secret")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "thread_id": "security-auth-bypass-1"
                })
                .to_string(),
            ))
            .unwrap();
        let resp = router.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn security_privilege_escalation_header_is_ignored() {
        let router = build_router(
            ExecutionApiState::new(build_test_graph().await).with_static_api_key_record_with_role(
                "sec-worker-1",
                "sec-worker-secret-1",
                true,
                ApiRole::Worker,
            ),
        );
        let req = Request::builder()
            .method(Method::POST)
            .uri("/v1/jobs/run")
            .header("x-oris-role", "admin")
            .header("x-api-key-id", "sec-worker-1")
            .header("x-api-key", "sec-worker-secret-1")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "thread_id": "security-rbac-escalation-1"
                })
                .to_string(),
            ))
            .unwrap();
        let resp = router.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .expect("forbidden body");
        let json: serde_json::Value = serde_json::from_slice(&body).expect("forbidden json");
        assert_eq!(json["error"]["details"]["role"], "worker");
    }

    #[tokio::test]
    async fn security_request_id_spoof_header_is_replaced() {
        let router = build_router(
            ExecutionApiState::new(build_test_graph().await).with_static_api_key("sec-api-key-1"),
        );
        let spoofed = "req injected value";
        let req = Request::builder()
            .method(Method::POST)
            .uri("/v1/jobs/run")
            .header("x-request-id", spoofed)
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "thread_id": "security-request-id-1"
                })
                .to_string(),
            ))
            .unwrap();
        let resp = router.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .expect("request id body");
        let json: serde_json::Value = serde_json::from_slice(&body).expect("request id json");
        let rid = json["request_id"].as_str().expect("request_id");
        assert_ne!(rid, spoofed);
        assert!(!rid.contains(' '));
    }

    #[cfg(feature = "sqlite-persistence")]
    #[tokio::test]
    async fn security_replay_resistance_idempotency_payload_swap_is_rejected() {
        let router = build_router(ExecutionApiState::with_sqlite_idempotency(
            build_test_graph().await,
            ":memory:",
        ));

        let first_req = Request::builder()
            .method(Method::POST)
            .uri("/v1/jobs/run")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "thread_id": "security-idem-1",
                    "input": "alpha",
                    "idempotency_key": "security-idem-key-1"
                })
                .to_string(),
            ))
            .unwrap();
        let first_resp = router.clone().oneshot(first_req).await.unwrap();
        assert_eq!(first_resp.status(), StatusCode::OK);

        let second_req = Request::builder()
            .method(Method::POST)
            .uri("/v1/jobs/run")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "thread_id": "security-idem-1",
                    "input": "beta",
                    "idempotency_key": "security-idem-key-1"
                })
                .to_string(),
            ))
            .unwrap();
        let second_resp = router.clone().oneshot(second_req).await.unwrap();
        assert_eq!(second_resp.status(), StatusCode::CONFLICT);

        let list_req = Request::builder()
            .method(Method::GET)
            .uri("/v1/jobs?limit=10&offset=0")
            .body(Body::empty())
            .unwrap();
        let list_resp = router.clone().oneshot(list_req).await.unwrap();
        assert_eq!(list_resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(list_resp.into_body(), usize::MAX)
            .await
            .expect("list body");
        let json: serde_json::Value = serde_json::from_slice(&body).expect("list json");
        let jobs = json["data"]["jobs"].as_array().expect("jobs array");
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0]["thread_id"], "security-idem-1");
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
    async fn replay_guard_dedupes_duplicate_replay_side_effects() {
        let effect_counter = Arc::new(AtomicUsize::new(0));
        let state = ExecutionApiState::with_sqlite_idempotency(
            build_side_effect_graph(Arc::clone(&effect_counter)).await,
            ":memory:",
        );
        let repo = state.runtime_repo.clone().expect("runtime repo");
        let router = build_router(state);

        let run_req = Request::builder()
            .method(Method::POST)
            .uri("/v1/jobs/run")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "thread_id": "replay-guard-1",
                    "input": "seed"
                })
                .to_string(),
            ))
            .unwrap();
        let run_resp = router.clone().oneshot(run_req).await.unwrap();
        assert_eq!(run_resp.status(), StatusCode::OK);
        assert_eq!(effect_counter.load(Ordering::SeqCst), 1);
        let replay_body = serde_json::json!({}).to_string();
        let first_replay_req = Request::builder()
            .method(Method::POST)
            .uri("/v1/jobs/replay-guard-1/replay")
            .header("content-type", "application/json")
            .body(Body::from(replay_body.clone()))
            .unwrap();
        let first_replay_resp = router.clone().oneshot(first_replay_req).await.unwrap();
        let first_replay_status = first_replay_resp.status();
        let first_replay_body = axum::body::to_bytes(first_replay_resp.into_body(), usize::MAX)
            .await
            .expect("first replay body");
        assert_eq!(
            first_replay_status,
            StatusCode::OK,
            "{}",
            String::from_utf8_lossy(&first_replay_body)
        );
        assert_eq!(effect_counter.load(Ordering::SeqCst), 2);

        let second_replay_req = Request::builder()
            .method(Method::POST)
            .uri("/v1/jobs/replay-guard-1/replay")
            .header("content-type", "application/json")
            .body(Body::from(replay_body))
            .unwrap();
        let second_replay_resp = router.oneshot(second_replay_req).await.unwrap();
        assert_eq!(second_replay_resp.status(), StatusCode::OK);
        let second_replay_body = axum::body::to_bytes(second_replay_resp.into_body(), usize::MAX)
            .await
            .expect("second replay body");
        let second_replay_json: serde_json::Value =
            serde_json::from_slice(&second_replay_body).expect("second replay json");
        assert_eq!(second_replay_json["data"]["idempotent_replay"], true);
        assert_eq!(effect_counter.load(Ordering::SeqCst), 2);

        let replay_effects = repo
            .list_replay_effects_for_thread("replay-guard-1")
            .expect("list replay effects");
        assert_eq!(replay_effects.len(), 1);
        assert_eq!(replay_effects[0].status, "completed");
        assert_eq!(replay_effects[0].execution_count, 1);
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
    async fn run_to_worker_flow_propagates_trace_context_end_to_end() {
        let state =
            ExecutionApiState::with_sqlite_idempotency(build_interrupt_graph().await, ":memory:");
        let repo = state.runtime_repo.clone().expect("runtime repo");
        let router = build_router(state);
        let incoming_traceparent = "00-0123456789abcdef0123456789abcdef-1111111111111111-01";

        let run_req = Request::builder()
            .method(Method::POST)
            .uri("/v1/jobs/run")
            .header("content-type", "application/json")
            .header("traceparent", incoming_traceparent)
            .body(Body::from(
                serde_json::json!({
                    "thread_id": "trace-run-1",
                    "input": "trace me"
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
        let run_trace = run_json["data"]["trace"].clone();
        assert_eq!(run_trace["trace_id"], "0123456789abcdef0123456789abcdef");
        assert_eq!(run_trace["parent_span_id"], "1111111111111111");
        let run_span_id = run_trace["span_id"].as_str().expect("run span").to_string();
        let expected_run_traceparent =
            format!("00-0123456789abcdef0123456789abcdef-{}-01", run_span_id);
        assert_eq!(run_trace["traceparent"], expected_run_traceparent.as_str());

        let poll_req = Request::builder()
            .method(Method::POST)
            .uri("/v1/workers/poll")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "worker_id": "trace-worker-1"
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
        let attempt_id = poll_json["data"]["attempt_id"]
            .as_str()
            .expect("attempt_id")
            .to_string();
        let lease_id = poll_json["data"]["lease_id"]
            .as_str()
            .expect("lease_id")
            .to_string();
        let poll_trace = poll_json["data"]["trace"].clone();
        assert_eq!(poll_trace["trace_id"], "0123456789abcdef0123456789abcdef");
        assert_eq!(poll_trace["parent_span_id"], run_span_id.as_str());
        let poll_span_id = poll_trace["span_id"]
            .as_str()
            .expect("poll span")
            .to_string();
        assert_ne!(poll_span_id, run_span_id);

        let hb_req = Request::builder()
            .method(Method::POST)
            .uri("/v1/workers/trace-worker-1/heartbeat")
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
        let hb_body = axum::body::to_bytes(hb_resp.into_body(), usize::MAX)
            .await
            .expect("heartbeat body");
        let hb_json: serde_json::Value = serde_json::from_slice(&hb_body).expect("heartbeat json");
        let hb_trace = hb_json["data"]["trace"].clone();
        assert_eq!(hb_trace["trace_id"], "0123456789abcdef0123456789abcdef");
        assert_eq!(hb_trace["parent_span_id"], poll_span_id.as_str());
        let hb_span_id = hb_trace["span_id"]
            .as_str()
            .expect("heartbeat span")
            .to_string();

        let ack_req = Request::builder()
            .method(Method::POST)
            .uri("/v1/workers/trace-worker-1/ack")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "attempt_id": attempt_id,
                    "terminal_status": "completed"
                })
                .to_string(),
            ))
            .unwrap();
        let ack_resp = router.clone().oneshot(ack_req).await.unwrap();
        assert_eq!(ack_resp.status(), StatusCode::OK);
        let ack_body = axum::body::to_bytes(ack_resp.into_body(), usize::MAX)
            .await
            .expect("ack body");
        let ack_json: serde_json::Value = serde_json::from_slice(&ack_body).expect("ack json");
        let ack_trace = ack_json["data"]["trace"].clone();
        assert_eq!(ack_trace["trace_id"], "0123456789abcdef0123456789abcdef");
        assert_eq!(ack_trace["parent_span_id"], hb_span_id.as_str());
        let ack_span_id = ack_trace["span_id"].as_str().expect("ack span").to_string();
        let persisted_trace = repo
            .latest_attempt_trace_for_run("trace-run-1")
            .expect("persisted trace query")
            .expect("persisted trace");
        assert_eq!(persisted_trace.trace_id, "0123456789abcdef0123456789abcdef");
        assert_eq!(
            persisted_trace.parent_span_id.as_deref(),
            Some(hb_span_id.as_str())
        );
        assert_eq!(persisted_trace.span_id, ack_span_id);
    }

    #[cfg(feature = "sqlite-persistence")]
    #[tokio::test]
    async fn run_job_rejects_invalid_traceparent_header() {
        let router = build_router(ExecutionApiState::with_sqlite_idempotency(
            build_test_graph().await,
            ":memory:",
        ));
        let req = Request::builder()
            .method(Method::POST)
            .uri("/v1/jobs/run")
            .header("content-type", "application/json")
            .header("traceparent", "00-invalid")
            .body(Body::from(
                serde_json::json!({
                    "thread_id": "trace-invalid-1",
                    "input": "bad trace"
                })
                .to_string(),
            ))
            .unwrap();
        let resp = router.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[cfg(feature = "sqlite-persistence")]
    #[tokio::test]
    async fn metrics_endpoint_is_scrape_ready_and_exposes_runtime_metrics() {
        let state =
            ExecutionApiState::with_sqlite_idempotency(build_test_graph().await, ":memory:")
                .with_static_api_key("metrics-key");
        let repo = state.runtime_repo.clone().expect("runtime repo");
        repo.enqueue_attempt("attempt-metrics-a", "run-metrics-1")
            .expect("enqueue a");
        repo.enqueue_attempt("attempt-metrics-b", "run-metrics-1")
            .expect("enqueue b");
        let router = build_router(state);

        let poll_req = Request::builder()
            .method(Method::POST)
            .uri("/v1/workers/poll")
            .header("content-type", "application/json")
            .header("x-api-key", "metrics-key")
            .body(Body::from(
                serde_json::json!({
                    "worker_id": "metrics-worker-1"
                })
                .to_string(),
            ))
            .unwrap();
        let poll_resp = router.clone().oneshot(poll_req).await.unwrap();
        assert_eq!(poll_resp.status(), StatusCode::OK);
        let poll_body = axum::body::to_bytes(poll_resp.into_body(), usize::MAX)
            .await
            .expect("metrics poll body");
        let poll_json: serde_json::Value =
            serde_json::from_slice(&poll_body).expect("metrics poll json");
        let attempt_id = poll_json["data"]["attempt_id"]
            .as_str()
            .expect("metrics attempt")
            .to_string();
        let lease_id = poll_json["data"]["lease_id"]
            .as_str()
            .expect("metrics lease")
            .to_string();

        let backpressure_req = Request::builder()
            .method(Method::POST)
            .uri("/v1/workers/poll")
            .header("content-type", "application/json")
            .header("x-api-key", "metrics-key")
            .body(Body::from(
                serde_json::json!({
                    "worker_id": "metrics-worker-1",
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
        assert_eq!(backpressure_json["data"]["reason"], "worker_limit");

        let wrong_hb_req = Request::builder()
            .method(Method::POST)
            .uri("/v1/workers/metrics-worker-other/heartbeat")
            .header("content-type", "application/json")
            .header("x-api-key", "metrics-key")
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
        .expect("force expire metrics lease");

        let recovery_poll_req = Request::builder()
            .method(Method::POST)
            .uri("/v1/workers/poll")
            .header("content-type", "application/json")
            .header("x-api-key", "metrics-key")
            .body(Body::from(
                serde_json::json!({
                    "worker_id": "metrics-worker-2"
                })
                .to_string(),
            ))
            .unwrap();
        let recovery_poll_resp = router.clone().oneshot(recovery_poll_req).await.unwrap();
        assert_eq!(recovery_poll_resp.status(), StatusCode::OK);
        let recovery_poll_body = axum::body::to_bytes(recovery_poll_resp.into_body(), usize::MAX)
            .await
            .expect("recovery poll body");
        let recovery_poll_json: serde_json::Value =
            serde_json::from_slice(&recovery_poll_body).expect("recovery poll json");
        assert_eq!(recovery_poll_json["data"]["decision"], "dispatched");

        let failed_ack_req = Request::builder()
            .method(Method::POST)
            .uri("/v1/workers/metrics-worker-2/ack")
            .header("content-type", "application/json")
            .header("x-api-key", "metrics-key")
            .body(Body::from(
                serde_json::json!({
                    "attempt_id": attempt_id,
                    "terminal_status": "failed"
                })
                .to_string(),
            ))
            .unwrap();
        let failed_ack_resp = router.clone().oneshot(failed_ack_req).await.unwrap();
        assert_eq!(failed_ack_resp.status(), StatusCode::OK);

        let metrics_req = Request::builder()
            .method(Method::GET)
            .uri("/metrics")
            .body(Body::empty())
            .unwrap();
        let metrics_resp = router.oneshot(metrics_req).await.unwrap();
        assert_eq!(metrics_resp.status(), StatusCode::OK);
        assert_eq!(
            metrics_resp
                .headers()
                .get("content-type")
                .and_then(|value| value.to_str().ok()),
            Some("text/plain; version=0.0.4; charset=utf-8")
        );
        let metrics_body = axum::body::to_bytes(metrics_resp.into_body(), usize::MAX)
            .await
            .expect("metrics body");
        let metrics_text = String::from_utf8(metrics_body.to_vec()).expect("metrics utf8");

        assert!(metrics_text.contains("# HELP oris_runtime_queue_depth"));
        assert!(metrics_text.contains("oris_runtime_queue_depth 1"));
        assert!(metrics_text.contains("oris_runtime_lease_operations_total 3"));
        assert!(metrics_text.contains("oris_runtime_lease_conflicts_total 1"));
        assert!(metrics_text.contains("oris_runtime_lease_conflict_rate 0.333333"));
        assert!(metrics_text.contains("oris_runtime_backpressure_total{reason=\"worker_limit\"} 1"));
        assert!(metrics_text.contains("oris_runtime_backpressure_total{reason=\"tenant_limit\"} 0"));
        assert!(metrics_text.contains("oris_runtime_terminal_acks_total{status=\"failed\"} 1"));
        assert!(metrics_text.contains("oris_runtime_terminal_error_rate 1.000000"));
        assert!(metrics_text.contains("oris_runtime_dispatch_latency_ms_count 2"));
        assert!(metrics_text.contains("oris_runtime_recovery_latency_ms_count 1"));
    }

    #[test]
    fn observability_assets_reference_metrics_present_in_sample_workload() {
        let dashboard = include_str!("../../../../../docs/observability/runtime-dashboard.json");
        let alerts = include_str!("../../../../../docs/observability/prometheus-alert-rules.yml");
        let sample = include_str!("../../../../../docs/observability/sample-runtime-workload.prom");

        let required_metrics = [
            "oris_runtime_queue_depth",
            "oris_runtime_backpressure_total",
            "oris_runtime_dispatch_latency_ms_bucket",
            "oris_runtime_recovery_latency_ms_bucket",
            "oris_runtime_terminal_error_rate",
            "oris_runtime_lease_conflict_rate",
        ];

        for metric in required_metrics {
            assert!(
                sample.contains(metric),
                "sample workload is missing metric {}",
                metric
            );
            assert!(
                dashboard.contains(metric),
                "dashboard is missing metric {}",
                metric
            );
        }

        let alert_metrics = [
            "oris_runtime_terminal_error_rate",
            "oris_runtime_recovery_latency_ms_bucket",
            "oris_runtime_backpressure_total",
            "oris_runtime_queue_depth",
        ];
        for metric in alert_metrics {
            assert!(
                alerts.contains(metric),
                "alert rules are missing metric {}",
                metric
            );
        }
    }

    #[cfg(feature = "sqlite-persistence")]
    #[tokio::test]
    async fn worker_failed_ack_schedules_retry_and_history_is_queryable() {
        let state =
            ExecutionApiState::with_sqlite_idempotency(build_test_graph().await, ":memory:");
        let repo = state.runtime_repo.clone().expect("runtime repo");
        repo.enqueue_attempt("attempt-worker-retry-1", "run-worker-retry-1")
            .expect("enqueue retry attempt");
        let router = build_router(state);

        let poll_req = Request::builder()
            .method(Method::POST)
            .uri("/v1/workers/poll")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "worker_id": "worker-retry-1"
                })
                .to_string(),
            ))
            .unwrap();
        let poll_resp = router.clone().oneshot(poll_req).await.unwrap();
        assert_eq!(poll_resp.status(), StatusCode::OK);

        let ack_req = Request::builder()
            .method(Method::POST)
            .uri("/v1/workers/worker-retry-1/ack")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "attempt_id": "attempt-worker-retry-1",
                    "terminal_status": "failed",
                    "retry_policy": {
                        "strategy": "fixed",
                        "backoff_ms": 1000,
                        "max_retries": 2
                    }
                })
                .to_string(),
            ))
            .unwrap();
        let ack_resp = router.clone().oneshot(ack_req).await.unwrap();
        assert_eq!(ack_resp.status(), StatusCode::OK);
        let ack_body = axum::body::to_bytes(ack_resp.into_body(), usize::MAX)
            .await
            .expect("ack body");
        let ack_json: serde_json::Value = serde_json::from_slice(&ack_body).expect("ack json");
        assert_eq!(ack_json["data"]["status"], "retry_scheduled");
        assert_eq!(ack_json["data"]["next_attempt_no"], 2);
        assert!(ack_json["data"]["next_retry_at"].is_string());

        let ready_now = repo
            .list_dispatchable_attempts(Utc::now(), 10)
            .expect("list dispatchable now");
        assert!(!ready_now
            .iter()
            .any(|row| row.attempt_id == "attempt-worker-retry-1"));

        let ready_later = repo
            .list_dispatchable_attempts(Utc::now() + Duration::seconds(2), 10)
            .expect("list dispatchable later");
        assert!(ready_later
            .iter()
            .any(|row| row.attempt_id == "attempt-worker-retry-1"));

        let history_req = Request::builder()
            .method(Method::GET)
            .uri("/v1/attempts/attempt-worker-retry-1/retries")
            .body(Body::empty())
            .unwrap();
        let history_resp = router.oneshot(history_req).await.unwrap();
        assert_eq!(history_resp.status(), StatusCode::OK);
        let history_body = axum::body::to_bytes(history_resp.into_body(), usize::MAX)
            .await
            .expect("history body");
        let history_json: serde_json::Value =
            serde_json::from_slice(&history_body).expect("history json");
        assert_eq!(history_json["data"]["current_status"], "retry_backoff");
        assert_eq!(history_json["data"]["current_attempt_no"], 2);
        assert_eq!(history_json["data"]["history"][0]["retry_no"], 1);
        assert_eq!(history_json["data"]["history"][0]["attempt_no"], 2);
        assert_eq!(history_json["data"]["history"][0]["strategy"], "fixed");
        assert_eq!(history_json["data"]["history"][0]["backoff_ms"], 1000);
    }

    #[cfg(feature = "sqlite-persistence")]
    #[tokio::test]
    async fn worker_poll_tick_transitions_timed_out_attempts() {
        let state =
            ExecutionApiState::with_sqlite_idempotency(build_test_graph().await, ":memory:");
        let repo = state.runtime_repo.clone().expect("runtime repo");
        repo.enqueue_attempt("attempt-worker-timeout-1", "run-worker-timeout-1")
            .expect("enqueue timeout attempt");
        repo.set_attempt_timeout_policy(
            "attempt-worker-timeout-1",
            &TimeoutPolicyConfig {
                timeout_ms: 1_000,
                on_timeout_status: AttemptExecutionStatus::Failed,
            },
        )
        .expect("set timeout policy");
        let router = build_router(state);

        let first_poll_req = Request::builder()
            .method(Method::POST)
            .uri("/v1/workers/poll")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "worker_id": "worker-timeout-1"
                })
                .to_string(),
            ))
            .unwrap();
        let first_poll_resp = router.clone().oneshot(first_poll_req).await.unwrap();
        assert_eq!(first_poll_resp.status(), StatusCode::OK);

        repo.set_attempt_started_at_for_test(
            "attempt-worker-timeout-1",
            Some(Utc::now() - Duration::seconds(5)),
        )
        .expect("backdate started_at");

        let second_poll_req = Request::builder()
            .method(Method::POST)
            .uri("/v1/workers/poll")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "worker_id": "worker-timeout-2"
                })
                .to_string(),
            ))
            .unwrap();
        let second_poll_resp = router.oneshot(second_poll_req).await.unwrap();
        assert_eq!(second_poll_resp.status(), StatusCode::OK);
        let second_poll_body = axum::body::to_bytes(second_poll_resp.into_body(), usize::MAX)
            .await
            .expect("second poll body");
        let second_poll_json: serde_json::Value =
            serde_json::from_slice(&second_poll_body).expect("second poll json");
        assert_eq!(second_poll_json["data"]["decision"], "noop");

        assert!(repo
            .get_lease_for_attempt("attempt-worker-timeout-1")
            .expect("read timeout lease")
            .is_none());
        let (_, status) = repo
            .get_attempt_status("attempt-worker-timeout-1")
            .expect("read timeout status")
            .expect("timeout attempt exists");
        assert_eq!(status, AttemptExecutionStatus::Failed);
    }

    #[cfg(feature = "sqlite-persistence")]
    #[tokio::test]
    async fn final_failed_attempts_are_visible_in_dlq_and_replayable_via_api() {
        let state =
            ExecutionApiState::with_sqlite_idempotency(build_test_graph().await, ":memory:");
        let repo = state.runtime_repo.clone().expect("runtime repo");
        repo.enqueue_attempt("attempt-dlq-api-1", "run-dlq-api-1")
            .expect("enqueue dlq api attempt");
        let router = build_router(state);

        let poll_req = Request::builder()
            .method(Method::POST)
            .uri("/v1/workers/poll")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "worker_id": "worker-dlq-api-1"
                })
                .to_string(),
            ))
            .unwrap();
        let poll_resp = router.clone().oneshot(poll_req).await.unwrap();
        assert_eq!(poll_resp.status(), StatusCode::OK);

        let ack_req = Request::builder()
            .method(Method::POST)
            .uri("/v1/workers/worker-dlq-api-1/ack")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "attempt_id": "attempt-dlq-api-1",
                    "terminal_status": "failed"
                })
                .to_string(),
            ))
            .unwrap();
        let ack_resp = router.clone().oneshot(ack_req).await.unwrap();
        assert_eq!(ack_resp.status(), StatusCode::OK);
        let ack_body = axum::body::to_bytes(ack_resp.into_body(), usize::MAX)
            .await
            .expect("ack body");
        let ack_json: serde_json::Value = serde_json::from_slice(&ack_body).expect("ack json");
        assert_eq!(ack_json["data"]["status"], "failed");

        let list_req = Request::builder()
            .method(Method::GET)
            .uri("/v1/dlq?status=pending")
            .body(Body::empty())
            .unwrap();
        let list_resp = router.clone().oneshot(list_req).await.unwrap();
        assert_eq!(list_resp.status(), StatusCode::OK);
        let list_body = axum::body::to_bytes(list_resp.into_body(), usize::MAX)
            .await
            .expect("dlq list body");
        let list_json: serde_json::Value =
            serde_json::from_slice(&list_body).expect("dlq list json");
        assert_eq!(
            list_json["data"]["entries"][0]["attempt_id"],
            "attempt-dlq-api-1"
        );
        assert_eq!(list_json["data"]["entries"][0]["replay_status"], "pending");

        let detail_req = Request::builder()
            .method(Method::GET)
            .uri("/v1/dlq/attempt-dlq-api-1")
            .body(Body::empty())
            .unwrap();
        let detail_resp = router.clone().oneshot(detail_req).await.unwrap();
        assert_eq!(detail_resp.status(), StatusCode::OK);
        let detail_body = axum::body::to_bytes(detail_resp.into_body(), usize::MAX)
            .await
            .expect("dlq detail body");
        let detail_json: serde_json::Value =
            serde_json::from_slice(&detail_body).expect("dlq detail json");
        assert_eq!(detail_json["data"]["terminal_status"], "failed");

        let replay_req = Request::builder()
            .method(Method::POST)
            .uri("/v1/dlq/attempt-dlq-api-1/replay")
            .body(Body::empty())
            .unwrap();
        let replay_resp = router.clone().oneshot(replay_req).await.unwrap();
        assert_eq!(replay_resp.status(), StatusCode::OK);
        let replay_body = axum::body::to_bytes(replay_resp.into_body(), usize::MAX)
            .await
            .expect("dlq replay body");
        let replay_json: serde_json::Value =
            serde_json::from_slice(&replay_body).expect("dlq replay json");
        assert_eq!(replay_json["data"]["status"], "requeued");
        assert_eq!(replay_json["data"]["replay_count"], 1);

        let replay_again_req = Request::builder()
            .method(Method::POST)
            .uri("/v1/dlq/attempt-dlq-api-1/replay")
            .body(Body::empty())
            .unwrap();
        let replay_again_resp = router.clone().oneshot(replay_again_req).await.unwrap();
        assert_eq!(replay_again_resp.status(), StatusCode::CONFLICT);

        let second_poll_req = Request::builder()
            .method(Method::POST)
            .uri("/v1/workers/poll")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "worker_id": "worker-dlq-api-2"
                })
                .to_string(),
            ))
            .unwrap();
        let second_poll_resp = router.oneshot(second_poll_req).await.unwrap();
        assert_eq!(second_poll_resp.status(), StatusCode::OK);
        let second_poll_body = axum::body::to_bytes(second_poll_resp.into_body(), usize::MAX)
            .await
            .expect("second poll body");
        let second_poll_json: serde_json::Value =
            serde_json::from_slice(&second_poll_body).expect("second poll json");
        assert_eq!(second_poll_json["data"]["decision"], "dispatched");
        assert_eq!(second_poll_json["data"]["attempt_id"], "attempt-dlq-api-1");

        let dlq_row = repo
            .get_dead_letter("attempt-dlq-api-1")
            .expect("read dlq row")
            .expect("dlq row exists");
        assert_eq!(dlq_row.replay_status, "replayed");
        assert_eq!(dlq_row.replay_count, 1);
    }

    #[cfg(feature = "sqlite-persistence")]
    #[tokio::test]
    async fn worker_poll_prefers_higher_priority_attempts() {
        let state =
            ExecutionApiState::with_sqlite_idempotency(build_test_graph().await, ":memory:");
        let repo = state.runtime_repo.clone().expect("runtime repo");
        repo.enqueue_attempt("attempt-priority-low", "run-priority-api")
            .expect("enqueue low priority");
        repo.enqueue_attempt("attempt-priority-high", "run-priority-api")
            .expect("enqueue high priority");
        repo.set_attempt_priority("attempt-priority-low", 5)
            .expect("set low priority");
        repo.set_attempt_priority("attempt-priority-high", 80)
            .expect("set high priority");
        let router = build_router(state);

        let first_poll_req = Request::builder()
            .method(Method::POST)
            .uri("/v1/workers/poll")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "worker_id": "worker-priority-1"
                })
                .to_string(),
            ))
            .unwrap();
        let first_poll_resp = router.clone().oneshot(first_poll_req).await.unwrap();
        assert_eq!(first_poll_resp.status(), StatusCode::OK);
        let first_poll_body = axum::body::to_bytes(first_poll_resp.into_body(), usize::MAX)
            .await
            .expect("first priority poll body");
        let first_poll_json: serde_json::Value =
            serde_json::from_slice(&first_poll_body).expect("first priority poll json");
        assert_eq!(
            first_poll_json["data"]["attempt_id"],
            "attempt-priority-high"
        );

        let second_poll_req = Request::builder()
            .method(Method::POST)
            .uri("/v1/workers/poll")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "worker_id": "worker-priority-2"
                })
                .to_string(),
            ))
            .unwrap();
        let second_poll_resp = router.oneshot(second_poll_req).await.unwrap();
        assert_eq!(second_poll_resp.status(), StatusCode::OK);
        let second_poll_body = axum::body::to_bytes(second_poll_resp.into_body(), usize::MAX)
            .await
            .expect("second priority poll body");
        let second_poll_json: serde_json::Value =
            serde_json::from_slice(&second_poll_body).expect("second priority poll json");
        assert_eq!(
            second_poll_json["data"]["attempt_id"],
            "attempt-priority-low"
        );
    }

    #[cfg(feature = "sqlite-persistence")]
    #[tokio::test]
    async fn auth_worker_role_cannot_access_dlq_endpoints() {
        let state =
            ExecutionApiState::with_sqlite_idempotency(build_test_graph().await, ":memory:")
                .with_static_api_key_record_with_role(
                    "worker-key-dlq",
                    "worker-secret-dlq",
                    true,
                    ApiRole::Worker,
                );
        let repo = state.runtime_repo.clone().expect("runtime repo");
        repo.enqueue_attempt("attempt-dlq-rbac", "run-dlq-rbac")
            .expect("enqueue dlq rbac attempt");
        repo.ack_attempt(
            "attempt-dlq-rbac",
            AttemptExecutionStatus::Failed,
            None,
            Utc::now(),
        )
        .expect("move attempt to dlq");
        let router = build_router(state);

        let list_req = Request::builder()
            .method(Method::GET)
            .uri("/v1/dlq")
            .header("x-api-key-id", "worker-key-dlq")
            .header("x-api-key", "worker-secret-dlq")
            .body(Body::empty())
            .unwrap();
        let list_resp = router.clone().oneshot(list_req).await.unwrap();
        assert_eq!(list_resp.status(), StatusCode::FORBIDDEN);

        let replay_req = Request::builder()
            .method(Method::POST)
            .uri("/v1/dlq/attempt-dlq-rbac/replay")
            .header("x-api-key-id", "worker-key-dlq")
            .header("x-api-key", "worker-secret-dlq")
            .body(Body::empty())
            .unwrap();
        let replay_resp = router.oneshot(replay_req).await.unwrap();
        assert_eq!(replay_resp.status(), StatusCode::FORBIDDEN);
    }

    #[cfg(feature = "sqlite-persistence")]
    #[tokio::test]
    async fn auth_worker_role_cannot_access_attempt_retry_history() {
        let state =
            ExecutionApiState::with_sqlite_idempotency(build_test_graph().await, ":memory:")
                .with_static_api_key_record_with_role(
                    "worker-key-retry",
                    "worker-secret-retry",
                    true,
                    ApiRole::Worker,
                );
        let repo = state.runtime_repo.clone().expect("runtime repo");
        repo.enqueue_attempt("attempt-worker-retry-rbac", "run-worker-retry-rbac")
            .expect("enqueue retry rbac attempt");
        let router = build_router(state);

        let req = Request::builder()
            .method(Method::GET)
            .uri("/v1/attempts/attempt-worker-retry-rbac/retries")
            .header("x-api-key-id", "worker-key-retry")
            .header("x-api-key", "worker-secret-retry")
            .body(Body::empty())
            .unwrap();
        let resp = router.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
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
        assert_eq!(backpressure_json["data"]["reason"], "worker_limit");
        assert_eq!(backpressure_json["data"]["worker_active_leases"], 1);
        assert_eq!(backpressure_json["data"]["worker_limit"], 1);

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
    async fn worker_poll_returns_tenant_backpressure_when_tenant_is_rate_limited() {
        let state =
            ExecutionApiState::with_sqlite_idempotency(build_test_graph().await, ":memory:");
        let repo = state.runtime_repo.clone().expect("runtime repo");
        repo.enqueue_attempt("attempt-tenant-1a", "run-tenant-1")
            .expect("enqueue tenant attempt a");
        repo.enqueue_attempt("attempt-tenant-1b", "run-tenant-1")
            .expect("enqueue tenant attempt b");
        repo.set_attempt_tenant_id("attempt-tenant-1a", Some("tenant-1"))
            .expect("set tenant a");
        repo.set_attempt_tenant_id("attempt-tenant-1b", Some("tenant-1"))
            .expect("set tenant b");
        let router = build_router(state);

        let first_poll_req = Request::builder()
            .method(Method::POST)
            .uri("/v1/workers/poll")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "worker_id": "worker-tenant-1",
                    "tenant_max_active_leases": 1
                })
                .to_string(),
            ))
            .unwrap();
        let first_poll_resp = router.clone().oneshot(first_poll_req).await.unwrap();
        assert_eq!(first_poll_resp.status(), StatusCode::OK);
        let first_poll_body = axum::body::to_bytes(first_poll_resp.into_body(), usize::MAX)
            .await
            .expect("first tenant poll body");
        let first_poll_json: serde_json::Value =
            serde_json::from_slice(&first_poll_body).expect("first tenant poll json");
        assert_eq!(first_poll_json["data"]["decision"], "dispatched");
        assert!(first_poll_json["data"]["attempt_id"].as_str().is_some());

        let second_poll_req = Request::builder()
            .method(Method::POST)
            .uri("/v1/workers/poll")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "worker_id": "worker-tenant-2",
                    "tenant_max_active_leases": 1
                })
                .to_string(),
            ))
            .unwrap();
        let second_poll_resp = router.oneshot(second_poll_req).await.unwrap();
        assert_eq!(second_poll_resp.status(), StatusCode::OK);
        let second_poll_body = axum::body::to_bytes(second_poll_resp.into_body(), usize::MAX)
            .await
            .expect("second tenant poll body");
        let second_poll_json: serde_json::Value =
            serde_json::from_slice(&second_poll_body).expect("second tenant poll json");
        assert_eq!(second_poll_json["data"]["decision"], "backpressure");
        assert_eq!(second_poll_json["data"]["reason"], "tenant_limit");
        assert_eq!(second_poll_json["data"]["tenant_id"], "tenant-1");
        assert_eq!(second_poll_json["data"]["tenant_active_leases"], 1);
        assert_eq!(second_poll_json["data"]["tenant_limit"], 1);
    }

    #[cfg(feature = "sqlite-persistence")]
    #[derive(Debug, Default)]
    struct SchedulerStressBaseline {
        dispatches: usize,
        conflict_injections: usize,
        conflicts_observed: usize,
        failover_injections: usize,
        failover_recoveries: usize,
        recovery_latency_ms: Vec<f64>,
        elapsed_seconds: f64,
    }

    #[cfg(feature = "sqlite-persistence")]
    impl SchedulerStressBaseline {
        fn conflict_rate(&self) -> f64 {
            if self.conflict_injections == 0 {
                0.0
            } else {
                self.conflicts_observed as f64 / self.conflict_injections as f64
            }
        }

        fn average_recovery_latency_ms(&self) -> f64 {
            if self.recovery_latency_ms.is_empty() {
                0.0
            } else {
                self.recovery_latency_ms.iter().sum::<f64>() / self.recovery_latency_ms.len() as f64
            }
        }

        fn max_recovery_latency_ms(&self) -> f64 {
            self.recovery_latency_ms
                .iter()
                .copied()
                .fold(0.0_f64, f64::max)
        }

        fn throughput_per_sec(&self) -> f64 {
            if self.elapsed_seconds <= f64::EPSILON {
                self.dispatches as f64
            } else {
                self.dispatches as f64 / self.elapsed_seconds
            }
        }
    }

    #[cfg(feature = "sqlite-persistence")]
    async fn poll_worker_json(
        router: &axum::Router,
        worker_id: &str,
        max_active_leases: usize,
        tenant_max_active_leases: usize,
    ) -> serde_json::Value {
        let req = Request::builder()
            .method(Method::POST)
            .uri("/v1/workers/poll")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "worker_id": worker_id,
                    "max_active_leases": max_active_leases,
                    "tenant_max_active_leases": tenant_max_active_leases
                })
                .to_string(),
            ))
            .unwrap();
        let resp = router.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .expect("poll body");
        serde_json::from_slice(&body).expect("poll json")
    }

    #[cfg(feature = "sqlite-persistence")]
    async fn heartbeat_status(
        router: &axum::Router,
        worker_id: &str,
        lease_id: &str,
        lease_ttl_seconds: i64,
    ) -> StatusCode {
        let req = Request::builder()
            .method(Method::POST)
            .uri(format!("/v1/workers/{worker_id}/heartbeat"))
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "lease_id": lease_id,
                    "lease_ttl_seconds": lease_ttl_seconds
                })
                .to_string(),
            ))
            .unwrap();
        router.clone().oneshot(req).await.unwrap().status()
    }

    #[cfg(feature = "sqlite-persistence")]
    async fn ack_completed_status(
        router: &axum::Router,
        worker_id: &str,
        attempt_id: &str,
    ) -> StatusCode {
        let req = Request::builder()
            .method(Method::POST)
            .uri(format!("/v1/workers/{worker_id}/ack"))
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "attempt_id": attempt_id,
                    "terminal_status": "completed"
                })
                .to_string(),
            ))
            .unwrap();
        router.clone().oneshot(req).await.unwrap().status()
    }

    #[cfg(feature = "sqlite-persistence")]
    async fn collect_scheduler_stress_baseline(
        iterations: usize,
        failover_every: usize,
    ) -> SchedulerStressBaseline {
        let state =
            ExecutionApiState::with_sqlite_idempotency(build_test_graph().await, ":memory:");
        let repo = state.runtime_repo.clone().expect("runtime repo");
        for i in 0..iterations {
            repo.enqueue_attempt(
                &format!("attempt-stress-{i}"),
                &format!("run-stress-{}", i / 2),
            )
            .expect("enqueue stress attempt");
            repo.set_attempt_tenant_id(
                &format!("attempt-stress-{i}"),
                Some(if i % 2 == 0 {
                    "tenant-alpha"
                } else {
                    "tenant-beta"
                }),
            )
            .expect("set stress tenant");
        }
        let router = build_router(state);
        let started = Instant::now();
        let mut baseline = SchedulerStressBaseline::default();

        for i in 0..iterations {
            let owner_worker_id = format!("stress-owner-{}", i % 4);
            let poll_json = poll_worker_json(&router, &owner_worker_id, 8, 2).await;
            assert_eq!(poll_json["data"]["decision"], "dispatched");
            baseline.dispatches += 1;

            let attempt_id = poll_json["data"]["attempt_id"]
                .as_str()
                .expect("stress attempt_id")
                .to_string();
            let lease_id = poll_json["data"]["lease_id"]
                .as_str()
                .expect("stress lease_id")
                .to_string();

            baseline.conflict_injections += 1;
            let conflict_status =
                heartbeat_status(&router, &format!("stress-conflict-{i}"), &lease_id, 5).await;
            assert_eq!(conflict_status, StatusCode::CONFLICT);
            baseline.conflicts_observed += 1;

            if failover_every > 0 && i % failover_every == 0 {
                baseline.failover_injections += 1;
                repo.heartbeat_lease(
                    &lease_id,
                    Utc::now() - Duration::seconds(40),
                    Utc::now() - Duration::seconds(20),
                )
                .expect("force expire lease");

                let recovery_start = Instant::now();
                let failover_worker_id = format!("stress-recovery-{i}");
                let failover_json = poll_worker_json(&router, &failover_worker_id, 8, 2).await;
                let recovery_latency_ms = recovery_start.elapsed().as_secs_f64() * 1000.0;

                assert_eq!(failover_json["data"]["decision"], "dispatched");
                assert_eq!(failover_json["data"]["attempt_id"], attempt_id);
                assert_ne!(failover_json["data"]["lease_id"], lease_id);

                baseline.dispatches += 1;
                baseline.failover_recoveries += 1;
                baseline.recovery_latency_ms.push(recovery_latency_ms);

                let ack_status =
                    ack_completed_status(&router, &failover_worker_id, &attempt_id).await;
                assert_eq!(ack_status, StatusCode::OK);
            } else {
                let ack_status = ack_completed_status(&router, &owner_worker_id, &attempt_id).await;
                assert_eq!(ack_status, StatusCode::OK);
            }
        }

        baseline.elapsed_seconds = started.elapsed().as_secs_f64();
        baseline
    }

    #[cfg(feature = "sqlite-persistence")]
    #[tokio::test]
    async fn scheduler_stress_conflict_injection_detects_ownership_mismatches() {
        let baseline = collect_scheduler_stress_baseline(12, 4).await;

        assert_eq!(baseline.conflicts_observed, baseline.conflict_injections);
        assert!(baseline.conflict_rate() >= 0.99);
    }

    #[cfg(feature = "sqlite-persistence")]
    #[tokio::test]
    async fn scheduler_stress_failover_recovers_after_forced_expiry() {
        let baseline = collect_scheduler_stress_baseline(12, 3).await;

        assert_eq!(baseline.failover_recoveries, baseline.failover_injections);
        assert!(baseline.average_recovery_latency_ms() >= 0.0);
        assert!(baseline.max_recovery_latency_ms() < 100.0);
    }

    #[cfg(feature = "sqlite-persistence")]
    #[tokio::test]
    async fn scheduler_stress_baseline_report_captures_conflict_latency_and_throughput() {
        let baseline = collect_scheduler_stress_baseline(24, 3).await;

        eprintln!(
            "scheduler_stress_baseline conflict_rate={:.2}% avg_recovery_latency_ms={:.3} max_recovery_latency_ms={:.3} throughput_ops_per_sec={:.2} dispatches={} failovers={}/{}",
            baseline.conflict_rate() * 100.0,
            baseline.average_recovery_latency_ms(),
            baseline.max_recovery_latency_ms(),
            baseline.throughput_per_sec(),
            baseline.dispatches,
            baseline.failover_recoveries,
            baseline.failover_injections,
        );

        assert!(baseline.conflict_rate() >= 0.99);
        assert_eq!(baseline.failover_recoveries, baseline.failover_injections);
        assert!(baseline.throughput_per_sec() > 1.0);
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
