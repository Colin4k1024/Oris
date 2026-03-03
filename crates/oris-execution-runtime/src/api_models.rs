//! API DTOs for Phase 2 execution server.

use crate::observability::KernelObservability;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct ApiEnvelope<T> {
    pub meta: ApiMeta,
    pub request_id: String,
    pub data: T,
}

#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct ApiMeta {
    pub status: &'static str,
    pub api_version: &'static str,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
pub struct TraceContextResponse {
    pub trace_id: String,
    pub span_id: String,
    pub parent_span_id: Option<String>,
    pub traceparent: String,
}

impl ApiMeta {
    pub fn ok() -> Self {
        Self {
            status: "ok",
            api_version: "v1",
        }
    }
}

#[derive(Clone, Debug, Deserialize, JsonSchema)]
pub struct RunJobRequest {
    pub thread_id: String,
    pub input: Option<String>,
    pub idempotency_key: Option<String>,
    pub timeout_policy: Option<TimeoutPolicyRequest>,
    pub priority: Option<i32>,
    pub tenant_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema)]
pub struct ResumeJobRequest {
    pub value: Value,
    pub checkpoint_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema)]
pub struct ReplayJobRequest {
    pub checkpoint_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema)]
pub struct CancelJobRequest {
    pub reason: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct RunJobResponse {
    pub thread_id: String,
    pub status: String,
    pub interrupts: Vec<Value>,
    pub idempotency_key: Option<String>,
    pub idempotent_replay: bool,
    pub trace: Option<TraceContextResponse>,
}

#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct JobStateResponse {
    pub thread_id: String,
    pub checkpoint_id: Option<String>,
    pub created_at: String,
    pub values: Value,
}

#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct JobHistoryItem {
    pub checkpoint_id: Option<String>,
    pub created_at: String,
}

#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct JobHistoryResponse {
    pub thread_id: String,
    pub history: Vec<JobHistoryItem>,
}

#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct JobTimelineItem {
    pub seq: u64,
    pub event_type: String,
    pub checkpoint_id: Option<String>,
    pub created_at: String,
}

#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct JobTimelineResponse {
    pub thread_id: String,
    pub timeline: Vec<JobTimelineItem>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observability: Option<KernelObservability>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trace: Option<TraceContextResponse>,
}

#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct CheckpointInspectResponse {
    pub thread_id: String,
    pub checkpoint_id: String,
    pub created_at: String,
    pub values: Value,
}

#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct CancelJobResponse {
    pub thread_id: String,
    pub status: String,
    pub reason: Option<String>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema)]
pub struct WorkerPollRequest {
    pub worker_id: String,
    pub limit: Option<usize>,
    pub max_active_leases: Option<usize>,
    pub tenant_max_active_leases: Option<usize>,
}

#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct WorkerPollResponse {
    pub decision: String,
    pub attempt_id: Option<String>,
    pub lease_id: Option<String>,
    pub lease_expires_at: Option<String>,
    pub reason: Option<String>,
    pub worker_active_leases: Option<usize>,
    pub worker_limit: Option<usize>,
    pub tenant_id: Option<String>,
    pub tenant_active_leases: Option<usize>,
    pub tenant_limit: Option<usize>,
    pub trace: Option<TraceContextResponse>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema)]
pub struct WorkerHeartbeatRequest {
    pub lease_id: String,
    pub lease_ttl_seconds: Option<i64>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema)]
pub struct WorkerExtendLeaseRequest {
    pub lease_id: String,
    pub lease_ttl_seconds: Option<i64>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema)]
pub struct WorkerReportStepRequest {
    pub attempt_id: String,
    pub action_id: String,
    pub status: String,
    pub dedupe_token: String,
}

#[derive(Clone, Debug, Deserialize, JsonSchema)]
pub struct WorkerAckRequest {
    pub attempt_id: String,
    pub terminal_status: String,
    pub retry_policy: Option<RetryPolicyRequest>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct RetryPolicyRequest {
    pub strategy: String,
    pub backoff_ms: i64,
    pub max_backoff_ms: Option<i64>,
    pub multiplier: Option<f64>,
    pub max_retries: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct TimeoutPolicyRequest {
    pub timeout_ms: i64,
    pub on_timeout_status: String,
}

#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct WorkerLeaseResponse {
    pub worker_id: String,
    pub lease_id: String,
    pub lease_expires_at: String,
    pub trace: Option<TraceContextResponse>,
}

#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct WorkerAckResponse {
    pub attempt_id: String,
    pub status: String,
    pub next_retry_at: Option<String>,
    pub next_attempt_no: Option<u32>,
    pub trace: Option<TraceContextResponse>,
}

#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct JobListItem {
    pub thread_id: String,
    pub status: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct ListJobsResponse {
    pub jobs: Vec<JobListItem>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema)]
pub struct ListJobsQuery {
    pub status: Option<String>,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}

#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct InterruptListItem {
    pub interrupt_id: String,
    pub thread_id: String,
    pub run_id: String,
    pub value: Value,
    pub status: String,
    pub created_at: String,
}

#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct InterruptListResponse {
    pub interrupts: Vec<InterruptListItem>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema)]
pub struct ListInterruptsQuery {
    pub status: Option<String>,
    pub run_id: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema)]
pub struct ListAuditLogsQuery {
    pub request_id: Option<String>,
    pub action: Option<String>,
    pub from_ms: Option<i64>,
    pub to_ms: Option<i64>,
    pub limit: Option<usize>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema)]
pub struct ListDeadLettersQuery {
    pub status: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Clone, Debug, Serialize, JsonSchema)]
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

#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct AuditLogListResponse {
    pub logs: Vec<AuditLogItem>,
}

#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct InterruptDetailResponse {
    pub interrupt_id: String,
    pub thread_id: String,
    pub run_id: String,
    pub attempt_id: String,
    pub value: Value,
    pub status: String,
    pub created_at: String,
}

#[derive(Clone, Debug, Deserialize, JsonSchema)]
pub struct ResumeInterruptRequest {
    pub value: Value,
}

#[derive(Clone, Debug, Deserialize, JsonSchema)]
pub struct RejectInterruptRequest {
    pub reason: Option<String>,
}

#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct JobDetailResponse {
    pub thread_id: String,
    pub status: String,
    pub checkpoint_id: Option<String>,
    pub values: Value,
    pub history: Vec<JobHistoryItem>,
    pub timeline: Vec<JobTimelineItem>,
    pub pending_interrupt: Option<InterruptDetailResponse>,
    pub trace: Option<TraceContextResponse>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observability: Option<KernelObservability>,
}

#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct TimelineExportResponse {
    pub thread_id: String,
    pub timeline: Vec<JobTimelineItem>,
    pub history: Vec<JobHistoryItem>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observability: Option<KernelObservability>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trace: Option<TraceContextResponse>,
}

#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct AttemptRetryHistoryItem {
    pub retry_no: u32,
    pub attempt_no: u32,
    pub strategy: String,
    pub backoff_ms: i64,
    pub max_retries: u32,
    pub scheduled_at: String,
}

#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct AttemptRetryHistoryResponse {
    pub attempt_id: String,
    pub current_attempt_no: u32,
    pub current_status: String,
    pub history: Vec<AttemptRetryHistoryItem>,
}

#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct DeadLetterItem {
    pub attempt_id: String,
    pub run_id: String,
    pub attempt_no: u32,
    pub terminal_status: String,
    pub reason: Option<String>,
    pub dead_at: String,
    pub replay_status: String,
    pub replay_count: u32,
    pub last_replayed_at: Option<String>,
}

#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct DeadLetterListResponse {
    pub entries: Vec<DeadLetterItem>,
}

#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct DeadLetterReplayResponse {
    pub attempt_id: String,
    pub status: String,
    pub replay_count: u32,
}
