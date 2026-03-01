//! API DTOs for Phase 2 execution server.

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, Serialize)]
pub struct ApiEnvelope<T> {
    pub meta: ApiMeta,
    pub request_id: String,
    pub data: T,
}

#[derive(Clone, Debug, Serialize)]
pub struct ApiMeta {
    pub status: &'static str,
    pub api_version: &'static str,
}

impl ApiMeta {
    pub fn ok() -> Self {
        Self {
            status: "ok",
            api_version: "v1",
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct RunJobRequest {
    pub thread_id: String,
    pub input: Option<String>,
    pub idempotency_key: Option<String>,
    pub timeout_policy: Option<TimeoutPolicyRequest>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ResumeJobRequest {
    pub value: Value,
    pub checkpoint_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ReplayJobRequest {
    pub checkpoint_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct CancelJobRequest {
    pub reason: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RunJobResponse {
    pub thread_id: String,
    pub status: String,
    pub interrupts: Vec<Value>,
    pub idempotency_key: Option<String>,
    pub idempotent_replay: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct JobStateResponse {
    pub thread_id: String,
    pub checkpoint_id: Option<String>,
    pub created_at: String,
    pub values: Value,
}

#[derive(Clone, Debug, Serialize)]
pub struct JobHistoryItem {
    pub checkpoint_id: Option<String>,
    pub created_at: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct JobHistoryResponse {
    pub thread_id: String,
    pub history: Vec<JobHistoryItem>,
}

#[derive(Clone, Debug, Serialize)]
pub struct JobTimelineItem {
    pub seq: u64,
    pub event_type: String,
    pub checkpoint_id: Option<String>,
    pub created_at: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct JobTimelineResponse {
    pub thread_id: String,
    pub timeline: Vec<JobTimelineItem>,
}

#[derive(Clone, Debug, Serialize)]
pub struct CheckpointInspectResponse {
    pub thread_id: String,
    pub checkpoint_id: String,
    pub created_at: String,
    pub values: Value,
}

#[derive(Clone, Debug, Serialize)]
pub struct CancelJobResponse {
    pub thread_id: String,
    pub status: String,
    pub reason: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct WorkerPollRequest {
    pub worker_id: String,
    pub limit: Option<usize>,
    pub max_active_leases: Option<usize>,
}

#[derive(Clone, Debug, Serialize)]
pub struct WorkerPollResponse {
    pub decision: String,
    pub attempt_id: Option<String>,
    pub lease_id: Option<String>,
    pub lease_expires_at: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct WorkerHeartbeatRequest {
    pub lease_id: String,
    pub lease_ttl_seconds: Option<i64>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct WorkerExtendLeaseRequest {
    pub lease_id: String,
    pub lease_ttl_seconds: Option<i64>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct WorkerReportStepRequest {
    pub attempt_id: String,
    pub action_id: String,
    pub status: String,
    pub dedupe_token: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct WorkerAckRequest {
    pub attempt_id: String,
    pub terminal_status: String,
    pub retry_policy: Option<RetryPolicyRequest>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RetryPolicyRequest {
    pub strategy: String,
    pub backoff_ms: i64,
    pub max_backoff_ms: Option<i64>,
    pub multiplier: Option<f64>,
    pub max_retries: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TimeoutPolicyRequest {
    pub timeout_ms: i64,
    pub on_timeout_status: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct WorkerLeaseResponse {
    pub worker_id: String,
    pub lease_id: String,
    pub lease_expires_at: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct WorkerAckResponse {
    pub attempt_id: String,
    pub status: String,
    pub next_retry_at: Option<String>,
    pub next_attempt_no: Option<u32>,
}

#[derive(Clone, Debug, Serialize)]
pub struct JobListItem {
    pub thread_id: String,
    pub status: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct ListJobsResponse {
    pub jobs: Vec<JobListItem>,
}

#[derive(Clone, Debug, Serialize)]
pub struct InterruptListItem {
    pub interrupt_id: String,
    pub thread_id: String,
    pub run_id: String,
    pub value: Value,
    pub status: String,
    pub created_at: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct InterruptListResponse {
    pub interrupts: Vec<InterruptListItem>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ListAuditLogsQuery {
    pub request_id: Option<String>,
    pub action: Option<String>,
    pub from_ms: Option<i64>,
    pub to_ms: Option<i64>,
    pub limit: Option<usize>,
}

#[derive(Clone, Debug, Serialize)]
pub struct AuditLogItem {
    pub audit_id: i64,
    pub actor_type: String,
    pub actor_id: Option<String>,
    pub actor_role: Option<String>,
    pub action: String,
    pub resource_type: String,
    pub resource_id: Option<String>,
    pub result: String,
    pub request_id: String,
    pub details: Option<Value>,
    pub created_at: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct AuditLogListResponse {
    pub logs: Vec<AuditLogItem>,
}

#[derive(Clone, Debug, Serialize)]
pub struct InterruptDetailResponse {
    pub interrupt_id: String,
    pub thread_id: String,
    pub run_id: String,
    pub attempt_id: String,
    pub value: Value,
    pub status: String,
    pub created_at: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ResumeInterruptRequest {
    pub value: Value,
}

#[derive(Clone, Debug, Deserialize)]
pub struct RejectInterruptRequest {
    pub reason: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct JobDetailResponse {
    pub thread_id: String,
    pub status: String,
    pub checkpoint_id: Option<String>,
    pub values: Value,
    pub history: Vec<JobHistoryItem>,
    pub timeline: Vec<JobTimelineItem>,
    pub pending_interrupt: Option<InterruptDetailResponse>,
}

#[derive(Clone, Debug, Serialize)]
pub struct TimelineExportResponse {
    pub thread_id: String,
    pub timeline: Vec<JobTimelineItem>,
    pub history: Vec<JobHistoryItem>,
}

#[derive(Clone, Debug, Serialize)]
pub struct AttemptRetryHistoryItem {
    pub retry_no: u32,
    pub attempt_no: u32,
    pub strategy: String,
    pub backoff_ms: i64,
    pub max_retries: u32,
    pub scheduled_at: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct AttemptRetryHistoryResponse {
    pub attempt_id: String,
    pub current_attempt_no: u32,
    pub current_status: String,
    pub history: Vec<AttemptRetryHistoryItem>,
}
