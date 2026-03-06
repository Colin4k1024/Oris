//! Axum handlers for Phase 2 execution server.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use axum::extract::{Path, Query, State};
use axum::http::{
    header::{AUTHORIZATION, CONTENT_TYPE},
    HeaderMap, StatusCode,
};
use axum::middleware::{from_fn, from_fn_with_state, Next};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{Duration, Utc};
use oris_execution_runtime::{
    ExecutionCheckpointView, ExecutionGraphBridge, ExecutionGraphBridgeErrorKind,
    KernelObservability,
};
use serde::de::DeserializeOwned;
use serde_json::Value;
use sha2::{Digest, Sha256};
use tokio::sync::RwLock;

#[cfg(all(
    feature = "agent-contract-experimental",
    feature = "evolution-network-experimental"
))]
use crate::agent_contract::{
    A2aCapability, A2aErrorCode, A2aErrorEnvelope, A2aHandshakeRequest, A2aHandshakeResponse,
    A2aProtocol, A2aTaskLifecycleEvent, A2aTaskLifecycleState, A2aTaskSessionAck,
    A2aTaskSessionCompletionRequest, A2aTaskSessionCompletionResponse,
    A2aTaskSessionDispatchRequest, A2aTaskSessionProgressItem, A2aTaskSessionProgressRequest,
    A2aTaskSessionResult, A2aTaskSessionSnapshot, A2aTaskSessionStartRequest, A2aTaskSessionState,
    AgentCapabilityLevel, AgentRole, ReplayFeedback, ReplayPlannerDirective,
    A2A_TASK_SESSION_PROTOCOL_VERSION,
};
#[cfg(feature = "evolution-network-experimental")]
use crate::evolution::{
    default_store_root, EvoEvolutionStore, EvolutionNetworkNode, ImportOutcome, JsonlEvolutionStore,
};
#[cfg(feature = "evolution-network-experimental")]
use crate::evolution_network::{FetchQuery, FetchResponse, PublishRequest, RevokeNotice};
use crate::execution_runtime::api_errors::ApiError;
#[cfg(feature = "sqlite-persistence")]
use crate::execution_runtime::api_idempotency::{IdempotencyRecord, SqliteIdempotencyStore};
use crate::execution_runtime::api_models::{
    ApiEnvelope, ApiMeta, AttemptRetryHistoryItem, AttemptRetryHistoryResponse, AuditLogItem,
    AuditLogListResponse, CancelJobRequest, CancelJobResponse, CheckpointInspectResponse,
    DeadLetterItem, DeadLetterListResponse, DeadLetterReplayResponse, InterruptDetailResponse,
    InterruptListItem, InterruptListResponse, JobDetailResponse, JobHistoryItem,
    JobHistoryResponse, JobListItem, JobStateResponse, JobTimelineItem, JobTimelineResponse,
    ListAuditLogsQuery, ListDeadLettersQuery, ListInterruptsQuery, ListJobsQuery, ListJobsResponse,
    RejectInterruptRequest, ReplayJobRequest, ResumeInterruptRequest, ResumeJobRequest,
    RetryPolicyRequest, RunJobRequest, RunJobResponse, TimelineExportResponse,
    TimeoutPolicyRequest, TraceContextResponse, WorkerAckRequest, WorkerAckResponse,
    WorkerExtendLeaseRequest, WorkerHeartbeatRequest, WorkerLeaseResponse, WorkerPollRequest,
    WorkerPollResponse, WorkerReportStepRequest,
};
use crate::execution_runtime::lease::{LeaseConfig, LeaseManager, RepositoryLeaseManager};
use crate::execution_runtime::models::AttemptExecutionStatus;
use crate::execution_runtime::repository::RuntimeRepository;
#[cfg(all(
    feature = "sqlite-persistence",
    feature = "agent-contract-experimental",
    feature = "evolution-network-experimental"
))]
use crate::execution_runtime::sqlite_runtime_repository::{A2aCompatTaskRow, A2aSessionRow};
#[cfg(feature = "sqlite-persistence")]
use crate::execution_runtime::sqlite_runtime_repository::{
    AttemptTraceContextRow, AuditLogEntry, DeadLetterRow, ReplayEffectClaim, RetryPolicyConfig,
    RetryStrategy, SqliteRuntimeRepository, StepReportWriteResult, TimeoutPolicyConfig,
};
use crate::graph::{CompiledGraph, MessagesState};
use tracing::{info_span, Instrument};

use super::graph_bridge::CompiledGraphExecutionBridge;

fn observability_and_trace_from_history(
    state: &ExecutionApiState,
    thread_id: &str,
    history: &[ExecutionCheckpointView],
) -> (Option<KernelObservability>, Option<TraceContextState>) {
    if history.is_empty() {
        return (None, None);
    }

    #[cfg(not(feature = "sqlite-persistence"))]
    let _ = state;

    #[cfg(feature = "sqlite-persistence")]
    {
        let trace = state
            .runtime_repo
            .as_ref()
            .and_then(|repo| repo.latest_attempt_trace_for_run(thread_id).ok().flatten())
            .map(TraceContextState::from_row);
        let lease_graph = state.runtime_repo.as_ref().and_then(|repo| {
            repo.latest_attempt_id_for_run(thread_id)
                .ok()
                .flatten()
                .and_then(|attempt_id| {
                    repo.get_lease_for_attempt(&attempt_id)
                        .ok()
                        .flatten()
                        .map(|lease| vec![(lease.attempt_id, lease.worker_id)])
                })
        });
        return (
            Some(
                KernelObservability::from_checkpoint_history_with_lease_graph(
                    thread_id,
                    history,
                    lease_graph,
                ),
            ),
            trace,
        );
    }

    #[cfg(not(feature = "sqlite-persistence"))]
    {
        (
            Some(KernelObservability::from_checkpoint_history(
                thread_id, history,
            )),
            None,
        )
    }
}

fn trace_response(trace: Option<TraceContextState>) -> Option<TraceContextResponse> {
    trace.map(|ctx| ctx.to_response())
}

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
    pub compat_node_secret_hash: Option<String>,
    pub compat_node_secret_role: ApiRole,
    pub keyed_api_keys: HashMap<String, StaticApiKeyConfig>,
}

#[derive(Clone, Debug)]
pub struct StaticApiKeyConfig {
    pub secret_hash: String,
    pub active: bool,
    pub role: ApiRole,
}

#[cfg(all(
    feature = "agent-contract-experimental",
    feature = "evolution-network-experimental"
))]
#[derive(Clone, Debug, PartialEq, Eq)]
struct A2aSessionPrincipal {
    actor_type: String,
    actor_id: Option<String>,
    actor_role: String,
}

#[cfg(all(
    feature = "agent-contract-experimental",
    feature = "evolution-network-experimental"
))]
#[derive(Clone, Debug)]
enum A2aPrivilegeProfile {
    Observer,
    Operator,
    Governor,
}

#[cfg(all(
    feature = "agent-contract-experimental",
    feature = "evolution-network-experimental"
))]
impl A2aPrivilegeProfile {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Observer => "observer",
            Self::Operator => "operator",
            Self::Governor => "governor",
        }
    }

    fn allows(&self, action: A2aPrivilegeAction) -> bool {
        match self {
            Self::Observer => matches!(
                action,
                A2aPrivilegeAction::EvolutionFetch
                    | A2aPrivilegeAction::TaskSessionSnapshot
                    | A2aPrivilegeAction::TaskLifecycleRead
            ),
            Self::Operator => matches!(
                action,
                A2aPrivilegeAction::EvolutionFetch
                    | A2aPrivilegeAction::EvolutionPublish
                    | A2aPrivilegeAction::TaskSessionStart
                    | A2aPrivilegeAction::TaskSessionDispatch
                    | A2aPrivilegeAction::TaskSessionProgress
                    | A2aPrivilegeAction::TaskSessionComplete
                    | A2aPrivilegeAction::TaskSessionSnapshot
                    | A2aPrivilegeAction::TaskLifecycleRead
            ),
            Self::Governor => true,
        }
    }
}

#[cfg(all(
    feature = "agent-contract-experimental",
    feature = "evolution-network-experimental"
))]
#[derive(Clone, Copy, Debug)]
enum A2aPrivilegeAction {
    EvolutionPublish,
    EvolutionFetch,
    EvolutionRevoke,
    TaskSessionStart,
    TaskSessionDispatch,
    TaskSessionProgress,
    TaskSessionComplete,
    TaskSessionSnapshot,
    TaskLifecycleRead,
}

#[cfg(all(
    feature = "agent-contract-experimental",
    feature = "evolution-network-experimental"
))]
impl A2aPrivilegeAction {
    fn as_str(&self) -> &'static str {
        match self {
            Self::EvolutionPublish => "evolution.publish",
            Self::EvolutionFetch => "evolution.fetch",
            Self::EvolutionRevoke => "evolution.revoke",
            Self::TaskSessionStart => "a2a.task_session.start",
            Self::TaskSessionDispatch => "a2a.task_session.dispatch",
            Self::TaskSessionProgress => "a2a.task_session.progress",
            Self::TaskSessionComplete => "a2a.task_session.complete",
            Self::TaskSessionSnapshot => "a2a.task_session.snapshot",
            Self::TaskLifecycleRead => "a2a.task_lifecycle.read",
        }
    }
}

#[cfg(all(
    feature = "agent-contract-experimental",
    feature = "evolution-network-experimental"
))]
#[derive(Clone, Debug)]
struct A2aSession {
    negotiated_protocol: A2aProtocol,
    enabled_capabilities: Vec<A2aCapability>,
    privilege_profile: A2aPrivilegeProfile,
    principal: Option<A2aSessionPrincipal>,
    expires_at: chrono::DateTime<Utc>,
}

#[cfg(all(
    feature = "agent-contract-experimental",
    feature = "evolution-network-experimental"
))]
const A2A_SESSION_TTL_HOURS: i64 = 24;

#[cfg(all(
    feature = "agent-contract-experimental",
    feature = "evolution-network-experimental"
))]
const A2A_TASK_EVENT_HISTORY_LIMIT: usize = 256;

#[cfg(all(
    feature = "agent-contract-experimental",
    feature = "evolution-network-experimental"
))]
const A2A_COMPAT_CLAIM_LEASE_MS: u64 = 60_000;

#[cfg(all(
    feature = "agent-contract-experimental",
    feature = "evolution-network-experimental"
))]
const GEP_A2A_PROTOCOL_NAME: &str = "gep-a2a";

#[cfg(all(
    feature = "agent-contract-experimental",
    feature = "evolution-network-experimental"
))]
const GEP_A2A_PROTOCOL_VERSION: &str = "1.0.0";

#[cfg(all(
    feature = "agent-contract-experimental",
    feature = "evolution-network-experimental"
))]
#[derive(Clone, Debug, serde::Deserialize)]
struct GepA2aEnvelope<T> {
    protocol: String,
    #[serde(default)]
    protocol_version: Option<String>,
    #[serde(default)]
    message_type: Option<String>,
    #[serde(default)]
    message_id: Option<String>,
    #[serde(default)]
    sender_id: Option<String>,
    #[serde(default)]
    node_id: Option<String>,
    #[serde(default)]
    timestamp: Option<String>,
    payload: Option<T>,
}

#[cfg(all(
    feature = "agent-contract-experimental",
    feature = "evolution-network-experimental"
))]
trait GepA2aEnvelopeCompatible {
    fn sender_id_mut(&mut self) -> &mut Option<String>;
    fn node_id_mut(&mut self) -> &mut Option<String>;
    fn protocol_version_mut(&mut self) -> &mut Option<String>;
}

#[cfg(all(
    feature = "agent-contract-experimental",
    feature = "evolution-network-experimental"
))]
fn parse_gep_envelope_or_plain<T>(
    raw: Value,
    expected_message_type: Option<&str>,
    rid: &str,
) -> Result<T, ApiError>
where
    T: DeserializeOwned + GepA2aEnvelopeCompatible,
{
    let maybe_obj = raw.as_object();
    let is_envelope = maybe_obj
        .map(|obj| obj.contains_key("protocol") && obj.contains_key("payload"))
        .unwrap_or(false);
    if !is_envelope {
        return serde_json::from_value(raw).map_err(|e| {
            ApiError::bad_request(format!("invalid compatibility payload: {e}"))
                .with_request_id(rid.to_string())
        });
    }

    let envelope: GepA2aEnvelope<Value> = serde_json::from_value(raw).map_err(|e| {
        ApiError::bad_request(format!("invalid gep-a2a envelope: {e}"))
            .with_request_id(rid.to_string())
    })?;
    if envelope.protocol != GEP_A2A_PROTOCOL_NAME {
        return Err(ApiError::bad_request("unsupported compatibility protocol")
            .with_request_id(rid.to_string())
            .with_details(serde_json::json!({
                "expected_protocol": GEP_A2A_PROTOCOL_NAME,
                "actual_protocol": envelope.protocol
            })));
    }
    let envelope_protocol_version = envelope
        .protocol_version
        .clone()
        .unwrap_or_else(|| GEP_A2A_PROTOCOL_VERSION.to_string());
    if envelope_protocol_version != GEP_A2A_PROTOCOL_VERSION {
        return Err(
            ApiError::bad_request("unsupported compatibility protocol version")
                .with_request_id(rid.to_string())
                .with_details(serde_json::json!({
                    "expected_protocol_version": GEP_A2A_PROTOCOL_VERSION,
                    "actual_protocol_version": envelope_protocol_version
                })),
        );
    }
    if let Some(expected_type) = expected_message_type {
        if envelope.message_type.as_deref() != Some(expected_type) {
            return Err(ApiError::bad_request("unexpected gep-a2a message_type")
                .with_request_id(rid.to_string())
                .with_details(serde_json::json!({
                    "expected_message_type": expected_type,
                    "actual_message_type": envelope.message_type
                })));
        }
    }

    let payload = envelope.payload.ok_or_else(|| {
        ApiError::bad_request("gep-a2a envelope payload is required")
            .with_request_id(rid.to_string())
    })?;
    let mut req: T = serde_json::from_value(payload).map_err(|e| {
        ApiError::bad_request(format!("invalid gep-a2a envelope payload: {e}"))
            .with_request_id(rid.to_string())
    })?;
    let sender = envelope.sender_id.or(envelope.node_id);
    if req
        .sender_id_mut()
        .as_ref()
        .map(|s| s.trim().is_empty())
        .unwrap_or(true)
    {
        *req.sender_id_mut() = sender.clone();
    }
    if req
        .node_id_mut()
        .as_ref()
        .map(|s| s.trim().is_empty())
        .unwrap_or(true)
    {
        *req.node_id_mut() = sender;
    }
    if req.protocol_version_mut().is_none() {
        *req.protocol_version_mut() = Some(envelope_protocol_version);
    }
    Ok(req)
}

#[cfg(all(
    feature = "agent-contract-experimental",
    feature = "evolution-network-experimental"
))]
fn parse_gep_hello_or_plain(raw: Value, rid: &str) -> Result<A2aHandshakeRequest, ApiError> {
    let maybe_obj = raw.as_object();
    let is_envelope = maybe_obj
        .map(|obj| obj.contains_key("protocol") && obj.contains_key("payload"))
        .unwrap_or(false);
    if !is_envelope {
        return serde_json::from_value(raw).map_err(|e| {
            ApiError::bad_request(format!("invalid hello payload: {e}"))
                .with_request_id(rid.to_string())
        });
    }

    let envelope: GepA2aEnvelope<Value> = serde_json::from_value(raw).map_err(|e| {
        ApiError::bad_request(format!("invalid gep-a2a hello envelope: {e}"))
            .with_request_id(rid.to_string())
    })?;
    if envelope.protocol != GEP_A2A_PROTOCOL_NAME {
        return Err(ApiError::bad_request("unsupported compatibility protocol")
            .with_request_id(rid.to_string())
            .with_details(serde_json::json!({
                "expected_protocol": GEP_A2A_PROTOCOL_NAME,
                "actual_protocol": envelope.protocol
            })));
    }
    let envelope_protocol_version = envelope
        .protocol_version
        .clone()
        .unwrap_or_else(|| GEP_A2A_PROTOCOL_VERSION.to_string());
    if envelope_protocol_version != GEP_A2A_PROTOCOL_VERSION {
        return Err(
            ApiError::bad_request("unsupported compatibility protocol version")
                .with_request_id(rid.to_string())
                .with_details(serde_json::json!({
                    "expected_protocol_version": GEP_A2A_PROTOCOL_VERSION,
                    "actual_protocol_version": envelope_protocol_version
                })),
        );
    }
    if envelope.message_type.as_deref() != Some("hello") {
        return Err(ApiError::bad_request("unexpected gep-a2a message_type")
            .with_request_id(rid.to_string())
            .with_details(serde_json::json!({
                "expected_message_type": "hello",
                "actual_message_type": envelope.message_type
            })));
    }
    let sender_id = envelope.sender_id.or(envelope.node_id).ok_or_else(|| {
        ApiError::bad_request("sender_id must be present in gep-a2a hello envelope")
            .with_request_id(rid.to_string())
    })?;
    let payload = envelope.payload.unwrap_or_else(|| serde_json::json!({}));
    let capabilities_from_payload = payload
        .get("capabilities")
        .and_then(|value| value.as_object())
        .map(|caps| {
            let mut mapped = Vec::new();
            if caps
                .get("coordination")
                .and_then(|value| value.as_bool())
                .unwrap_or(false)
            {
                mapped.push(A2aCapability::Coordination);
            }
            if caps
                .get("supervised_devloop")
                .and_then(|value| value.as_bool())
                .unwrap_or(false)
            {
                mapped.push(A2aCapability::SupervisedDevloop);
            }
            if caps
                .get("replay_feedback")
                .and_then(|value| value.as_bool())
                .unwrap_or(false)
            {
                mapped.push(A2aCapability::ReplayFeedback);
            }
            if caps
                .get("evolution_fetch")
                .and_then(|value| value.as_bool())
                .unwrap_or(false)
            {
                mapped.push(A2aCapability::EvolutionFetch);
            }
            if caps
                .get("evolution_publish")
                .and_then(|value| value.as_bool())
                .unwrap_or(false)
            {
                mapped.push(A2aCapability::EvolutionPublish);
            }
            if caps
                .get("evolution_revoke")
                .and_then(|value| value.as_bool())
                .unwrap_or(false)
            {
                mapped.push(A2aCapability::EvolutionRevoke);
            }
            mapped
        })
        .unwrap_or_default();
    let advertised_capabilities = if capabilities_from_payload.is_empty() {
        vec![
            A2aCapability::Coordination,
            A2aCapability::SupervisedDevloop,
            A2aCapability::ReplayFeedback,
            A2aCapability::EvolutionFetch,
        ]
    } else {
        capabilities_from_payload
    };

    Ok(A2aHandshakeRequest {
        agent_id: sender_id,
        role: AgentRole::Planner,
        capability_level: AgentCapabilityLevel::A4,
        supported_protocols: vec![
            A2aProtocol {
                name: crate::agent_contract::A2A_PROTOCOL_NAME.to_string(),
                version: crate::agent_contract::A2A_PROTOCOL_VERSION_V1.to_string(),
            },
            A2aProtocol {
                name: crate::agent_contract::A2A_PROTOCOL_NAME.to_string(),
                version: crate::agent_contract::A2A_TASK_SESSION_PROTOCOL_VERSION.to_string(),
            },
        ],
        advertised_capabilities,
    })
}

#[cfg(all(
    feature = "agent-contract-experimental",
    feature = "evolution-network-experimental"
))]
#[derive(Clone, Debug, serde::Serialize)]
pub struct A2aTaskLifecycleResponse {
    task_id: String,
    events: Vec<A2aTaskLifecycleEvent>,
}

#[cfg(all(
    feature = "agent-contract-experimental",
    feature = "evolution-network-experimental"
))]
#[derive(Clone, Debug, serde::Deserialize)]
pub struct A2aTaskSessionLookupQuery {
    sender_id: String,
    protocol_version: String,
}

#[cfg(all(
    feature = "agent-contract-experimental",
    feature = "evolution-network-experimental"
))]
#[derive(Clone, Debug, serde::Deserialize)]
pub struct A2aSessionReplicationExportQuery {
    protocol_version: String,
}

#[cfg(all(
    feature = "agent-contract-experimental",
    feature = "evolution-network-experimental"
))]
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct A2aSessionReplicationPayload {
    sender_id: String,
    protocol: A2aProtocol,
    enabled_capabilities: Vec<A2aCapability>,
    actor_type: Option<String>,
    actor_id: Option<String>,
    actor_role: Option<String>,
    expires_at_ms: u64,
}

#[cfg(all(
    feature = "agent-contract-experimental",
    feature = "evolution-network-experimental"
))]
#[derive(Clone, Debug, serde::Deserialize)]
pub struct A2aSessionReplicationImportRequest {
    source_node_id: String,
    protocol_version: String,
    session: A2aSessionReplicationPayload,
}

#[cfg(all(
    feature = "agent-contract-experimental",
    feature = "evolution-network-experimental"
))]
#[derive(Clone, Debug, serde::Serialize)]
pub struct A2aSessionReplicationResponse {
    imported: bool,
    source_node_id: String,
    sender_id: String,
    expires_at_ms: u64,
}

#[cfg(all(
    feature = "agent-contract-experimental",
    feature = "evolution-network-experimental"
))]
#[derive(Clone, Debug, serde::Deserialize)]
pub struct A2aCompatDistributeRequest {
    sender_id: Option<String>,
    #[serde(default)]
    node_id: Option<String>,
    #[serde(default)]
    protocol_version: Option<String>,
    task_id: String,
    #[serde(alias = "task_description")]
    task_summary: String,
    dispatch_id: Option<String>,
    summary: Option<String>,
}

#[cfg(all(
    feature = "agent-contract-experimental",
    feature = "evolution-network-experimental"
))]
#[derive(Clone, Debug, serde::Serialize)]
pub struct A2aCompatDistributeResponse {
    session_id: String,
    task_id: String,
    state: A2aTaskSessionState,
    summary: String,
}

#[cfg(all(
    feature = "agent-contract-experimental",
    feature = "evolution-network-experimental"
))]
#[derive(Clone, Debug, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum A2aCompatReportStatus {
    #[serde(alias = "in_progress")]
    Running,
    #[serde(alias = "completed", alias = "success")]
    Succeeded,
    #[serde(alias = "error")]
    Failed,
    #[serde(alias = "canceled")]
    Cancelled,
}

#[cfg(all(
    feature = "agent-contract-experimental",
    feature = "evolution-network-experimental"
))]
#[derive(Clone, Debug, serde::Deserialize)]
pub struct A2aCompatReportRequest {
    sender_id: Option<String>,
    #[serde(default)]
    node_id: Option<String>,
    #[serde(default)]
    protocol_version: Option<String>,
    task_id: String,
    status: A2aCompatReportStatus,
    summary: String,
    progress_pct: Option<u8>,
    retryable: Option<bool>,
    retry_after_ms: Option<u64>,
    failure_code: Option<A2aErrorCode>,
    failure_details: Option<String>,
    used_capsule: Option<bool>,
    capsule_id: Option<String>,
    reasoning_steps_avoided: Option<u64>,
    fallback_reason: Option<String>,
    task_class_id: Option<String>,
    task_label: Option<String>,
}

#[cfg(all(
    feature = "agent-contract-experimental",
    feature = "evolution-network-experimental"
))]
#[derive(Clone, Debug, serde::Serialize)]
pub struct A2aCompatReportResponse {
    session_id: String,
    task_id: String,
    state: A2aTaskSessionState,
    terminal_state: Option<A2aTaskLifecycleState>,
    summary: String,
}

#[cfg(all(
    feature = "agent-contract-experimental",
    feature = "evolution-network-experimental"
))]
#[derive(Clone, Debug, serde::Deserialize)]
pub struct A2aCompatTaskCompleteRequest {
    sender_id: Option<String>,
    #[serde(default)]
    node_id: Option<String>,
    #[serde(default)]
    protocol_version: Option<String>,
    task_id: String,
    #[serde(default)]
    status: Option<A2aCompatReportStatus>,
    #[serde(default)]
    success: Option<bool>,
    summary: Option<String>,
    #[serde(default)]
    retryable: Option<bool>,
    #[serde(default)]
    retry_after_ms: Option<u64>,
    #[serde(default)]
    failure_code: Option<A2aErrorCode>,
    #[serde(default)]
    failure_details: Option<String>,
    #[serde(default)]
    used_capsule: Option<bool>,
    #[serde(default)]
    capsule_id: Option<String>,
    #[serde(default)]
    reasoning_steps_avoided: Option<u64>,
    #[serde(default)]
    fallback_reason: Option<String>,
    #[serde(default)]
    task_class_id: Option<String>,
    #[serde(default)]
    task_label: Option<String>,
}

#[cfg(all(
    feature = "agent-contract-experimental",
    feature = "evolution-network-experimental"
))]
#[derive(Clone, Debug, serde::Deserialize)]
pub struct A2aCompatWorkClaimRequest {
    sender_id: Option<String>,
    #[serde(default)]
    node_id: Option<String>,
    #[serde(default)]
    worker_id: Option<String>,
    #[serde(default)]
    protocol_version: Option<String>,
}

#[cfg(all(
    feature = "agent-contract-experimental",
    feature = "evolution-network-experimental"
))]
#[derive(Clone, Debug, serde::Serialize)]
pub struct A2aCompatWorkAssignment {
    assignment_id: String,
    task_id: String,
    task_summary: String,
    dispatch_id: String,
}

#[cfg(all(
    feature = "agent-contract-experimental",
    feature = "evolution-network-experimental"
))]
#[derive(Clone, Debug, serde::Serialize)]
pub struct A2aCompatWorkClaimResponse {
    claimed: bool,
    assignment: Option<A2aCompatWorkAssignment>,
    retry_after_ms: Option<u64>,
}

#[cfg(all(
    feature = "agent-contract-experimental",
    feature = "evolution-network-experimental"
))]
#[derive(Clone, Debug, serde::Deserialize)]
pub struct A2aCompatWorkCompleteRequest {
    sender_id: Option<String>,
    #[serde(default)]
    node_id: Option<String>,
    #[serde(default)]
    worker_id: Option<String>,
    #[serde(default)]
    protocol_version: Option<String>,
    assignment_id: String,
    #[serde(default)]
    task_id: Option<String>,
    #[serde(default)]
    status: Option<A2aCompatReportStatus>,
    #[serde(default)]
    success: Option<bool>,
    summary: Option<String>,
    #[serde(default)]
    retryable: Option<bool>,
    #[serde(default)]
    retry_after_ms: Option<u64>,
    #[serde(default)]
    failure_code: Option<A2aErrorCode>,
    #[serde(default)]
    failure_details: Option<String>,
    #[serde(default)]
    used_capsule: Option<bool>,
    #[serde(default)]
    capsule_id: Option<String>,
    #[serde(default)]
    reasoning_steps_avoided: Option<u64>,
    #[serde(default)]
    fallback_reason: Option<String>,
    #[serde(default)]
    task_class_id: Option<String>,
    #[serde(default)]
    task_label: Option<String>,
}

#[cfg(all(
    feature = "agent-contract-experimental",
    feature = "evolution-network-experimental"
))]
#[derive(Clone, Debug, serde::Serialize)]
pub struct A2aCompatWorkCompleteResponse {
    assignment_id: String,
    task_id: String,
    state: A2aTaskSessionState,
    terminal_state: Option<A2aTaskLifecycleState>,
    summary: String,
}

#[cfg(all(
    feature = "agent-contract-experimental",
    feature = "evolution-network-experimental"
))]
#[derive(Clone, Debug, serde::Deserialize)]
pub struct A2aCompatHeartbeatRequest {
    sender_id: Option<String>,
    #[serde(default)]
    node_id: Option<String>,
    #[serde(default)]
    worker_id: Option<String>,
    #[serde(default)]
    protocol_version: Option<String>,
    #[serde(default)]
    metadata: Option<serde_json::Value>,
}

#[cfg(all(
    feature = "agent-contract-experimental",
    feature = "evolution-network-experimental"
))]
#[derive(Clone, Debug, serde::Serialize)]
pub struct A2aCompatAvailableWorkItem {
    assignment_id: String,
    task_id: String,
    task_summary: String,
    dispatch_id: String,
}

#[cfg(all(
    feature = "agent-contract-experimental",
    feature = "evolution-network-experimental"
))]
#[derive(Clone, Debug, serde::Serialize)]
pub struct A2aCompatHeartbeatResponse {
    acknowledged: bool,
    worker_id: String,
    available_work_count: usize,
    available_work: Vec<A2aCompatAvailableWorkItem>,
    metadata_accepted: bool,
}

#[cfg(all(
    feature = "agent-contract-experimental",
    feature = "evolution-network-experimental"
))]
impl GepA2aEnvelopeCompatible for A2aCompatDistributeRequest {
    fn sender_id_mut(&mut self) -> &mut Option<String> {
        &mut self.sender_id
    }

    fn node_id_mut(&mut self) -> &mut Option<String> {
        &mut self.node_id
    }

    fn protocol_version_mut(&mut self) -> &mut Option<String> {
        &mut self.protocol_version
    }
}

#[cfg(all(
    feature = "agent-contract-experimental",
    feature = "evolution-network-experimental"
))]
impl GepA2aEnvelopeCompatible for A2aCompatFetchRequest {
    fn sender_id_mut(&mut self) -> &mut Option<String> {
        &mut self.sender_id
    }

    fn node_id_mut(&mut self) -> &mut Option<String> {
        &mut self.node_id
    }

    fn protocol_version_mut(&mut self) -> &mut Option<String> {
        &mut self.protocol_version
    }
}

#[cfg(all(
    feature = "agent-contract-experimental",
    feature = "evolution-network-experimental"
))]
impl GepA2aEnvelopeCompatible for A2aCompatClaimRequest {
    fn sender_id_mut(&mut self) -> &mut Option<String> {
        &mut self.sender_id
    }

    fn node_id_mut(&mut self) -> &mut Option<String> {
        &mut self.node_id
    }

    fn protocol_version_mut(&mut self) -> &mut Option<String> {
        &mut self.protocol_version
    }
}

#[cfg(all(
    feature = "agent-contract-experimental",
    feature = "evolution-network-experimental"
))]
impl GepA2aEnvelopeCompatible for A2aCompatReportRequest {
    fn sender_id_mut(&mut self) -> &mut Option<String> {
        &mut self.sender_id
    }

    fn node_id_mut(&mut self) -> &mut Option<String> {
        &mut self.node_id
    }

    fn protocol_version_mut(&mut self) -> &mut Option<String> {
        &mut self.protocol_version
    }
}

#[cfg(all(
    feature = "agent-contract-experimental",
    feature = "evolution-network-experimental"
))]
impl GepA2aEnvelopeCompatible for A2aCompatTaskCompleteRequest {
    fn sender_id_mut(&mut self) -> &mut Option<String> {
        &mut self.sender_id
    }

    fn node_id_mut(&mut self) -> &mut Option<String> {
        &mut self.node_id
    }

    fn protocol_version_mut(&mut self) -> &mut Option<String> {
        &mut self.protocol_version
    }
}

#[cfg(all(
    feature = "agent-contract-experimental",
    feature = "evolution-network-experimental"
))]
impl GepA2aEnvelopeCompatible for A2aCompatWorkClaimRequest {
    fn sender_id_mut(&mut self) -> &mut Option<String> {
        &mut self.sender_id
    }

    fn node_id_mut(&mut self) -> &mut Option<String> {
        &mut self.node_id
    }

    fn protocol_version_mut(&mut self) -> &mut Option<String> {
        &mut self.protocol_version
    }
}

#[cfg(all(
    feature = "agent-contract-experimental",
    feature = "evolution-network-experimental"
))]
impl GepA2aEnvelopeCompatible for A2aCompatWorkCompleteRequest {
    fn sender_id_mut(&mut self) -> &mut Option<String> {
        &mut self.sender_id
    }

    fn node_id_mut(&mut self) -> &mut Option<String> {
        &mut self.node_id
    }

    fn protocol_version_mut(&mut self) -> &mut Option<String> {
        &mut self.protocol_version
    }
}

#[cfg(all(
    feature = "agent-contract-experimental",
    feature = "evolution-network-experimental"
))]
impl GepA2aEnvelopeCompatible for A2aCompatHeartbeatRequest {
    fn sender_id_mut(&mut self) -> &mut Option<String> {
        &mut self.sender_id
    }

    fn node_id_mut(&mut self) -> &mut Option<String> {
        &mut self.node_id
    }

    fn protocol_version_mut(&mut self) -> &mut Option<String> {
        &mut self.protocol_version
    }
}

#[cfg(all(
    feature = "agent-contract-experimental",
    feature = "evolution-network-experimental"
))]
#[derive(Clone, Debug, serde::Deserialize)]
pub struct A2aCompatClaimRequest {
    sender_id: Option<String>,
    #[serde(default)]
    node_id: Option<String>,
    #[serde(default)]
    protocol_version: Option<String>,
}

#[cfg(all(
    feature = "agent-contract-experimental",
    feature = "evolution-network-experimental"
))]
#[derive(Clone, Debug, serde::Serialize)]
pub struct A2aCompatClaimTask {
    session_id: String,
    task_id: String,
    task_summary: String,
    dispatch_id: String,
}

#[cfg(all(
    feature = "agent-contract-experimental",
    feature = "evolution-network-experimental"
))]
#[derive(Clone, Debug, serde::Serialize)]
pub struct A2aCompatClaimResponse {
    claimed: bool,
    task: Option<A2aCompatClaimTask>,
    retry_after_ms: Option<u64>,
}

#[cfg(all(
    feature = "agent-contract-experimental",
    feature = "evolution-network-experimental"
))]
#[derive(Clone, Debug, serde::Deserialize)]
pub struct A2aCompatFetchRequest {
    sender_id: Option<String>,
    #[serde(default)]
    node_id: Option<String>,
    #[serde(default)]
    protocol_version: Option<String>,
    #[serde(default)]
    signals: Vec<String>,
    #[serde(default)]
    include_tasks: bool,
}

#[cfg(all(
    feature = "agent-contract-experimental",
    feature = "evolution-network-experimental"
))]
#[derive(Clone, Debug, serde::Serialize)]
pub struct A2aCompatFetchTask {
    session_id: String,
    task_id: String,
    task_summary: String,
    dispatch_id: String,
    claimable: bool,
    lease_expires_at_ms: Option<u64>,
}

#[cfg(all(
    feature = "agent-contract-experimental",
    feature = "evolution-network-experimental"
))]
#[derive(Clone, Debug, serde::Serialize)]
pub struct A2aCompatFetchResponse {
    sender_id: String,
    assets: Vec<crate::evolution_network::NetworkAsset>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tasks: Option<Vec<A2aCompatFetchTask>>,
}

#[cfg(all(
    feature = "agent-contract-experimental",
    feature = "evolution-network-experimental"
))]
#[derive(Clone, Debug)]
struct A2aCompatQueueEntry {
    session_id: String,
    owner_sender_id: String,
    protocol_version: String,
    task_id: String,
    task_summary: String,
    dispatch_id: String,
    claimed_by: Option<String>,
    lease_expires_at_ms: Option<u64>,
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
    a2a_task_lease_expired_total: u64,
    a2a_task_claim_latency_ms_sum: f64,
    a2a_task_claim_latency_ms_count: u64,
    a2a_task_claim_latency_ms_buckets: [u64; METRIC_BUCKETS_MS.len()],
    a2a_report_to_capture_latency_ms_sum: f64,
    a2a_report_to_capture_latency_ms_count: u64,
    a2a_report_to_capture_latency_ms_buckets: [u64; METRIC_BUCKETS_MS.len()],
    a2a_fetch_total: u64,
    a2a_task_claim_total: u64,
    a2a_task_complete_total: u64,
    a2a_work_claim_total: u64,
    a2a_work_complete_total: u64,
    a2a_heartbeat_total: u64,
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
    a2a_task_lease_expired_total: u64,
    a2a_task_claim_latency_ms_sum: f64,
    a2a_task_claim_latency_ms_count: u64,
    a2a_task_claim_latency_ms_buckets: [u64; METRIC_BUCKETS_MS.len()],
    a2a_report_to_capture_latency_ms_sum: f64,
    a2a_report_to_capture_latency_ms_count: u64,
    a2a_report_to_capture_latency_ms_buckets: [u64; METRIC_BUCKETS_MS.len()],
    a2a_fetch_total: u64,
    a2a_task_claim_total: u64,
    a2a_task_complete_total: u64,
    a2a_work_claim_total: u64,
    a2a_work_complete_total: u64,
    a2a_heartbeat_total: u64,
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
                a2a_task_lease_expired_total: 0,
                a2a_task_claim_latency_ms_sum: 0.0,
                a2a_task_claim_latency_ms_count: 0,
                a2a_task_claim_latency_ms_buckets: [0; METRIC_BUCKETS_MS.len()],
                a2a_report_to_capture_latency_ms_sum: 0.0,
                a2a_report_to_capture_latency_ms_count: 0,
                a2a_report_to_capture_latency_ms_buckets: [0; METRIC_BUCKETS_MS.len()],
                a2a_fetch_total: 0,
                a2a_task_claim_total: 0,
                a2a_task_complete_total: 0,
                a2a_work_claim_total: 0,
                a2a_work_complete_total: 0,
                a2a_heartbeat_total: 0,
            })),
        }
    }
}

impl RuntimeMetrics {
    fn record_a2a_fetch(&self) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.a2a_fetch_total += 1;
        }
    }

    fn record_a2a_task_claim(&self) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.a2a_task_claim_total += 1;
        }
    }

    fn record_a2a_task_complete(&self) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.a2a_task_complete_total += 1;
        }
    }

    fn record_a2a_work_claim(&self) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.a2a_work_claim_total += 1;
        }
    }

    fn record_a2a_work_complete(&self) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.a2a_work_complete_total += 1;
        }
    }

    fn record_a2a_heartbeat(&self) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.a2a_heartbeat_total += 1;
        }
    }

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

    fn record_a2a_task_lease_expired(&self) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.a2a_task_lease_expired_total += 1;
        }
    }

    fn record_a2a_task_claim_latency_ms(&self, latency_ms: f64) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.a2a_task_claim_latency_ms_sum += latency_ms;
            inner.a2a_task_claim_latency_ms_count += 1;
            record_histogram_bucket(&mut inner.a2a_task_claim_latency_ms_buckets, latency_ms);
        }
    }

    fn record_a2a_report_to_capture_latency_ms(&self, latency_ms: f64) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.a2a_report_to_capture_latency_ms_sum += latency_ms;
            inner.a2a_report_to_capture_latency_ms_count += 1;
            record_histogram_bucket(
                &mut inner.a2a_report_to_capture_latency_ms_buckets,
                latency_ms,
            );
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
                a2a_task_lease_expired_total: inner.a2a_task_lease_expired_total,
                a2a_task_claim_latency_ms_sum: inner.a2a_task_claim_latency_ms_sum,
                a2a_task_claim_latency_ms_count: inner.a2a_task_claim_latency_ms_count,
                a2a_task_claim_latency_ms_buckets: inner.a2a_task_claim_latency_ms_buckets,
                a2a_report_to_capture_latency_ms_sum: inner.a2a_report_to_capture_latency_ms_sum,
                a2a_report_to_capture_latency_ms_count: inner
                    .a2a_report_to_capture_latency_ms_count,
                a2a_report_to_capture_latency_ms_buckets: inner
                    .a2a_report_to_capture_latency_ms_buckets,
                a2a_fetch_total: inner.a2a_fetch_total,
                a2a_task_claim_total: inner.a2a_task_claim_total,
                a2a_task_complete_total: inner.a2a_task_complete_total,
                a2a_work_claim_total: inner.a2a_work_claim_total,
                a2a_work_complete_total: inner.a2a_work_complete_total,
                a2a_heartbeat_total: inner.a2a_heartbeat_total,
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
                a2a_task_lease_expired_total: 0,
                a2a_task_claim_latency_ms_sum: 0.0,
                a2a_task_claim_latency_ms_count: 0,
                a2a_task_claim_latency_ms_buckets: [0; METRIC_BUCKETS_MS.len()],
                a2a_report_to_capture_latency_ms_sum: 0.0,
                a2a_report_to_capture_latency_ms_count: 0,
                a2a_report_to_capture_latency_ms_buckets: [0; METRIC_BUCKETS_MS.len()],
                a2a_fetch_total: 0,
                a2a_task_claim_total: 0,
                a2a_task_complete_total: 0,
                a2a_work_claim_total: 0,
                a2a_work_complete_total: 0,
                a2a_heartbeat_total: 0,
            }
        }
    }

    fn render_prometheus(&self, queue_depth: usize, a2a_task_queue_depth: usize) -> String {
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
        out.push_str("# HELP oris_a2a_task_queue_depth Number of compatibility A2A tasks currently queued.\n");
        out.push_str("# TYPE oris_a2a_task_queue_depth gauge\n");
        out.push_str(&format!(
            "oris_a2a_task_queue_depth {}\n",
            a2a_task_queue_depth
        ));
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
        out.push_str("# HELP oris_a2a_task_lease_expired_total Number of compat A2A task leases reclaimed after expiry.\n");
        out.push_str("# TYPE oris_a2a_task_lease_expired_total counter\n");
        out.push_str(&format!(
            "oris_a2a_task_lease_expired_total {}\n",
            snapshot.a2a_task_lease_expired_total
        ));
        render_histogram(
            &mut out,
            "oris_a2a_task_claim_latency_ms",
            "Compatibility A2A task claim latency in milliseconds.",
            &snapshot.a2a_task_claim_latency_ms_buckets,
            snapshot.a2a_task_claim_latency_ms_count,
            snapshot.a2a_task_claim_latency_ms_sum,
        );
        render_histogram(
            &mut out,
            "oris_a2a_report_to_capture_latency_ms",
            "Compatibility A2A terminal report to capture latency in milliseconds.",
            &snapshot.a2a_report_to_capture_latency_ms_buckets,
            snapshot.a2a_report_to_capture_latency_ms_count,
            snapshot.a2a_report_to_capture_latency_ms_sum,
        );
        out.push_str(
            "# HELP oris_a2a_fetch_total Total compatibility /a2a/fetch requests served.\n",
        );
        out.push_str("# TYPE oris_a2a_fetch_total counter\n");
        out.push_str(&format!(
            "oris_a2a_fetch_total {}\n",
            snapshot.a2a_fetch_total
        ));
        out.push_str("# HELP oris_a2a_task_claim_total Total compatibility /a2a/task(s)/claim requests served.\n");
        out.push_str("# TYPE oris_a2a_task_claim_total counter\n");
        out.push_str(&format!(
            "oris_a2a_task_claim_total {}\n",
            snapshot.a2a_task_claim_total
        ));
        out.push_str(
            "# HELP oris_a2a_task_complete_total Total compatibility /a2a/task/complete requests served.\n",
        );
        out.push_str("# TYPE oris_a2a_task_complete_total counter\n");
        out.push_str(&format!(
            "oris_a2a_task_complete_total {}\n",
            snapshot.a2a_task_complete_total
        ));
        out.push_str(
            "# HELP oris_a2a_work_claim_total Total compatibility /a2a/work/claim requests served.\n",
        );
        out.push_str("# TYPE oris_a2a_work_claim_total counter\n");
        out.push_str(&format!(
            "oris_a2a_work_claim_total {}\n",
            snapshot.a2a_work_claim_total
        ));
        out.push_str("# HELP oris_a2a_work_complete_total Total compatibility /a2a/work/complete requests served.\n");
        out.push_str("# TYPE oris_a2a_work_complete_total counter\n");
        out.push_str(&format!(
            "oris_a2a_work_complete_total {}\n",
            snapshot.a2a_work_complete_total
        ));
        out.push_str(
            "# HELP oris_a2a_heartbeat_total Total compatibility /a2a/heartbeat requests served.\n",
        );
        out.push_str("# TYPE oris_a2a_heartbeat_total counter\n");
        out.push_str(&format!(
            "oris_a2a_heartbeat_total {}\n",
            snapshot.a2a_heartbeat_total
        ));
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
            compat_node_secret_hash: None,
            compat_node_secret_role: ApiRole::Operator,
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
            || self.compat_node_secret_hash.is_some()
            || !self.keyed_api_keys.is_empty()
    }
}

#[derive(Clone)]
pub struct ExecutionApiState {
    pub compiled: Arc<CompiledGraph<MessagesState>>,
    pub graph_bridge: Arc<dyn ExecutionGraphBridge>,
    pub cancelled_threads: Arc<RwLock<HashSet<String>>>,
    #[cfg(feature = "evolution-network-experimental")]
    pub evolution_store: Arc<dyn EvoEvolutionStore>,
    #[cfg(feature = "evolution-network-experimental")]
    pub evolution_node: Arc<EvolutionNetworkNode>,
    #[cfg(all(
        feature = "agent-contract-experimental",
        feature = "evolution-network-experimental"
    ))]
    a2a_sessions: Arc<RwLock<HashMap<String, A2aSession>>>,
    #[cfg(all(
        feature = "agent-contract-experimental",
        feature = "evolution-network-experimental"
    ))]
    a2a_task_lifecycle_events: Arc<RwLock<HashMap<String, Vec<A2aTaskLifecycleEvent>>>>,
    #[cfg(all(
        feature = "agent-contract-experimental",
        feature = "evolution-network-experimental"
    ))]
    a2a_task_sessions: Arc<RwLock<HashMap<String, A2aTaskSessionSnapshot>>>,
    #[cfg(all(
        feature = "agent-contract-experimental",
        feature = "evolution-network-experimental"
    ))]
    a2a_compat_task_queue: Arc<RwLock<VecDeque<A2aCompatQueueEntry>>>,
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
        #[cfg(feature = "evolution-network-experimental")]
        let evolution_store: Arc<dyn EvoEvolutionStore> =
            Arc::new(JsonlEvolutionStore::new(default_store_root()));

        Self {
            graph_bridge: Arc::new(CompiledGraphExecutionBridge::new(compiled.clone())),
            compiled,
            cancelled_threads: Arc::new(RwLock::new(HashSet::new())),
            #[cfg(feature = "evolution-network-experimental")]
            evolution_store: evolution_store.clone(),
            #[cfg(feature = "evolution-network-experimental")]
            evolution_node: Arc::new(EvolutionNetworkNode::new(evolution_store)),
            #[cfg(all(
                feature = "agent-contract-experimental",
                feature = "evolution-network-experimental"
            ))]
            a2a_sessions: Arc::new(RwLock::new(HashMap::new())),
            #[cfg(all(
                feature = "agent-contract-experimental",
                feature = "evolution-network-experimental"
            ))]
            a2a_task_lifecycle_events: Arc::new(RwLock::new(HashMap::new())),
            #[cfg(all(
                feature = "agent-contract-experimental",
                feature = "evolution-network-experimental"
            ))]
            a2a_task_sessions: Arc::new(RwLock::new(HashMap::new())),
            #[cfg(all(
                feature = "agent-contract-experimental",
                feature = "evolution-network-experimental"
            ))]
            a2a_compat_task_queue: Arc::new(RwLock::new(VecDeque::new())),
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

    pub fn with_graph_bridge(mut self, graph_bridge: Arc<dyn ExecutionGraphBridge>) -> Self {
        self.graph_bridge = graph_bridge;
        self
    }

    #[cfg(feature = "evolution-network-experimental")]
    pub fn with_evolution_store(mut self, store: Arc<dyn EvoEvolutionStore>) -> Self {
        self.evolution_node = Arc::new(EvolutionNetworkNode::new(store.clone()));
        self.evolution_store = store;
        self
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

    pub fn with_compat_node_secret(mut self, secret: impl Into<String>) -> Self {
        self.auth.compat_node_secret_hash =
            ExecutionApiAuthConfig::normalize_hashed_secret(Some(secret.into()));
        self.auth.compat_node_secret_role = ApiRole::Operator;
        self
    }

    pub fn with_compat_node_secret_with_role(
        mut self,
        secret: impl Into<String>,
        role: ApiRole,
    ) -> Self {
        self.auth.compat_node_secret_hash =
            ExecutionApiAuthConfig::normalize_hashed_secret(Some(secret.into()));
        self.auth.compat_node_secret_role = role;
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
    let secured = with_evolution_routes(
        Router::new()
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
            ),
    )
    .layer(from_fn_with_state(state.clone(), auth_middleware))
    .layer(from_fn(request_log_middleware))
    .layer(from_fn_with_state(state.clone(), audit_middleware))
    .with_state(state.clone());

    Router::new()
        .route("/healthz", get(healthz_endpoint))
        .route("/metrics", get(metrics_endpoint))
        .with_state(state)
        .merge(secured)
}

#[cfg(feature = "evolution-network-experimental")]
fn with_evolution_routes(router: Router<ExecutionApiState>) -> Router<ExecutionApiState> {
    let router = router
        .route("/v1/evolution/publish", post(evolution_publish))
        .route("/v1/evolution/fetch", post(evolution_fetch))
        .route("/v1/evolution/revoke", post(evolution_revoke));
    #[cfg(feature = "agent-contract-experimental")]
    let router = router
        .route("/v1/evolution/a2a/handshake", post(evolution_a2a_handshake))
        .route("/a2a/hello", post(evolution_a2a_hello_compat))
        .route("/a2a/fetch", post(evolution_a2a_fetch_compat))
        .route(
            "/a2a/tasks/distribute",
            post(evolution_a2a_tasks_distribute_compat),
        )
        .route("/a2a/task/claim", post(evolution_a2a_tasks_claim_compat))
        .route(
            "/a2a/task/complete",
            post(evolution_a2a_task_complete_compat),
        )
        .route("/a2a/work/claim", post(evolution_a2a_work_claim_compat))
        .route(
            "/a2a/work/complete",
            post(evolution_a2a_work_complete_compat),
        )
        .route("/a2a/heartbeat", post(evolution_a2a_heartbeat_compat))
        .route("/a2a/tasks/claim", post(evolution_a2a_tasks_claim_compat))
        .route("/a2a/tasks/report", post(evolution_a2a_tasks_report_compat))
        .route("/evolution/a2a/hello", post(evolution_a2a_hello))
        .route(
            "/evolution/a2a/tasks/distribute",
            post(evolution_a2a_tasks_distribute),
        )
        .route(
            "/evolution/a2a/tasks/claim",
            post(evolution_a2a_tasks_claim),
        )
        .route(
            "/evolution/a2a/tasks/report",
            post(evolution_a2a_tasks_report),
        )
        .route(
            "/v1/evolution/a2a/sessions/start",
            post(evolution_a2a_session_start),
        )
        .route(
            "/v1/evolution/a2a/sessions/:session_id/dispatch",
            post(evolution_a2a_session_dispatch),
        )
        .route(
            "/v1/evolution/a2a/sessions/:session_id/progress",
            post(evolution_a2a_session_progress),
        )
        .route(
            "/v1/evolution/a2a/sessions/:session_id/complete",
            post(evolution_a2a_session_complete),
        )
        .route(
            "/v1/evolution/a2a/sessions/:sender_id/replicate",
            get(evolution_a2a_export_session),
        )
        .route(
            "/v1/evolution/a2a/sessions/replicate",
            post(evolution_a2a_import_session),
        )
        .route(
            "/v1/evolution/a2a/sessions/:session_id",
            get(evolution_a2a_session_snapshot),
        )
        .route(
            "/v1/evolution/a2a/tasks/:task_id/lifecycle",
            get(evolution_a2a_task_lifecycle),
        );
    router
}

#[cfg(not(feature = "evolution-network-experimental"))]
fn with_evolution_routes(router: Router<ExecutionApiState>) -> Router<ExecutionApiState> {
    router
}

async fn healthz_endpoint(
    State(_state): State<ExecutionApiState>,
) -> Result<impl IntoResponse, ApiError> {
    #[cfg(feature = "evolution-network-experimental")]
    let evolution =
        Some(_state.evolution_node.health_snapshot().map_err(|err| {
            ApiError::internal(format!("failed to inspect evolution health: {err}"))
        })?);
    #[cfg(not(feature = "evolution-network-experimental"))]
    let evolution: Option<serde_json::Value> = None;

    Ok((
        StatusCode::OK,
        Json(serde_json::json!({
            "status": "ok",
            "evolution": evolution,
        })),
    ))
}

async fn metrics_endpoint(
    State(state): State<ExecutionApiState>,
) -> Result<impl IntoResponse, ApiError> {
    #[cfg(feature = "sqlite-persistence")]
    let queue_depth = state
        .runtime_repo
        .as_ref()
        .and_then(|repo| repo.queue_depth(Utc::now()).ok())
        .unwrap_or(0);
    #[cfg(not(feature = "sqlite-persistence"))]
    let queue_depth = 0usize;

    #[cfg(all(
        feature = "agent-contract-experimental",
        feature = "evolution-network-experimental"
    ))]
    let a2a_task_queue_depth = {
        #[cfg(feature = "sqlite-persistence")]
        {
            if let Some(repo) = state.runtime_repo.as_ref() {
                match repo.a2a_compat_queue_depth() {
                    Ok(depth) => depth,
                    Err(_) => state.a2a_compat_task_queue.read().await.len(),
                }
            } else {
                state.a2a_compat_task_queue.read().await.len()
            }
        }
        #[cfg(not(feature = "sqlite-persistence"))]
        {
            state.a2a_compat_task_queue.read().await.len()
        }
    };
    #[cfg(not(all(
        feature = "agent-contract-experimental",
        feature = "evolution-network-experimental"
    )))]
    let a2a_task_queue_depth = 0usize;

    let body = state
        .runtime_metrics
        .render_prometheus(queue_depth, a2a_task_queue_depth);
    #[cfg(feature = "evolution-network-experimental")]
    let mut body = body;
    #[cfg(feature = "evolution-network-experimental")]
    {
        let evolution_metrics =
            state
                .evolution_node
                .render_metrics_prometheus()
                .map_err(|err| {
                    ApiError::internal(format!("failed to render evolution metrics: {err}"))
                })?;
        if !body.ends_with('\n') {
            body.push('\n');
        }
        body.push_str(&evolution_metrics);
    }

    Ok((
        [(CONTENT_TYPE, "text/plain; version=0.0.4; charset=utf-8")],
        body,
    ))
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
    let snapshot = match state.graph_bridge.snapshot(thread_id, None).await {
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

fn compat_node_id_from_headers(headers: &HeaderMap) -> Option<String> {
    headers
        .get("x-node-id")
        .or_else(|| headers.get("x-sender-id"))
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(|v| v.to_string())
}

fn is_a2a_compat_path(path: &str) -> bool {
    path.starts_with("/a2a/")
        || path.starts_with("/evolution/a2a/")
        || path.starts_with("/v1/evolution/a2a/")
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
        .compat_node_secret_hash
        .as_deref()
        .zip(bearer_token_from_headers(headers))
        .map(|(expected_hash, actual)| expected_hash == ExecutionApiAuthConfig::secret_hash(actual))
        .unwrap_or(false)
    {
        return Some(AuthContext {
            actor_type: "node_secret".to_string(),
            actor_id: compat_node_id_from_headers(headers),
            role: auth.compat_node_secret_role.clone(),
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

#[cfg(all(
    feature = "agent-contract-experimental",
    feature = "evolution-network-experimental"
))]
fn resolve_a2a_principal(
    headers: &HeaderMap,
    state: &ExecutionApiState,
) -> Option<A2aSessionPrincipal> {
    resolve_auth_context(headers, state).map(|auth| A2aSessionPrincipal {
        actor_type: auth.actor_type,
        actor_id: auth.actor_id,
        actor_role: auth.role.as_str().to_string(),
    })
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
    if seg.is_empty() {
        return None;
    }
    if seg[0] == "a2a" {
        return match seg.as_slice() {
            ["a2a", "hello"] => Some(AuditTarget {
                action: "a2a.compat.hello",
                resource_type: "sender",
                resource_id: None,
            }),
            ["a2a", "fetch"] => Some(AuditTarget {
                action: "a2a.compat.fetch",
                resource_type: "task",
                resource_id: None,
            }),
            ["a2a", "tasks", "distribute"] => Some(AuditTarget {
                action: "a2a.compat.distribute",
                resource_type: "task",
                resource_id: None,
            }),
            ["a2a", "tasks", "claim"] | ["a2a", "task", "claim"] => Some(AuditTarget {
                action: "a2a.compat.claim",
                resource_type: "task",
                resource_id: None,
            }),
            ["a2a", "tasks", "report"] | ["a2a", "task", "complete"] => Some(AuditTarget {
                action: "a2a.compat.report",
                resource_type: "task",
                resource_id: None,
            }),
            ["a2a", "work", "claim"] => Some(AuditTarget {
                action: "a2a.compat.work.claim",
                resource_type: "assignment",
                resource_id: None,
            }),
            ["a2a", "work", "complete"] => Some(AuditTarget {
                action: "a2a.compat.work.complete",
                resource_type: "assignment",
                resource_id: None,
            }),
            ["a2a", "heartbeat"] => Some(AuditTarget {
                action: "a2a.compat.heartbeat",
                resource_type: "worker",
                resource_id: None,
            }),
            _ => None,
        };
    }
    if seg[0] == "evolution" {
        return match seg.as_slice() {
            ["evolution", "a2a", "hello"] => Some(AuditTarget {
                action: "a2a.compat.hello",
                resource_type: "sender",
                resource_id: None,
            }),
            ["evolution", "a2a", "tasks", "distribute"] => Some(AuditTarget {
                action: "a2a.compat.distribute",
                resource_type: "task",
                resource_id: None,
            }),
            ["evolution", "a2a", "tasks", "claim"] => Some(AuditTarget {
                action: "a2a.compat.claim",
                resource_type: "task",
                resource_id: None,
            }),
            ["evolution", "a2a", "tasks", "report"] => Some(AuditTarget {
                action: "a2a.compat.report",
                resource_type: "task",
                resource_id: None,
            }),
            _ => None,
        };
    }
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
        ("evolution", ["v1", "evolution", "publish"]) => Some(AuditTarget {
            action: "evolution.publish",
            resource_type: "sender",
            resource_id: None,
        }),
        ("evolution", ["v1", "evolution", "fetch"]) => Some(AuditTarget {
            action: "evolution.fetch",
            resource_type: "sender",
            resource_id: None,
        }),
        ("evolution", ["v1", "evolution", "revoke"]) => Some(AuditTarget {
            action: "evolution.revoke",
            resource_type: "sender",
            resource_id: None,
        }),
        ("evolution", ["v1", "evolution", "a2a", "sessions", "start"]) => Some(AuditTarget {
            action: "a2a.task_session.start",
            resource_type: "session",
            resource_id: None,
        }),
        ("evolution", ["v1", "evolution", "a2a", "sessions", session_id, "dispatch"]) => {
            Some(AuditTarget {
                action: "a2a.task_session.dispatch",
                resource_type: "session",
                resource_id: Some((*session_id).to_string()),
            })
        }
        ("evolution", ["v1", "evolution", "a2a", "sessions", session_id, "progress"]) => {
            Some(AuditTarget {
                action: "a2a.task_session.progress",
                resource_type: "session",
                resource_id: Some((*session_id).to_string()),
            })
        }
        ("evolution", ["v1", "evolution", "a2a", "sessions", session_id, "complete"]) => {
            Some(AuditTarget {
                action: "a2a.task_session.complete",
                resource_type: "session",
                resource_id: Some((*session_id).to_string()),
            })
        }
        ("evolution", ["v1", "evolution", "a2a", "sessions", "replicate"]) => Some(AuditTarget {
            action: "a2a.session.replicate.import",
            resource_type: "sender",
            resource_id: None,
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
    let is_a2a_compat = is_a2a_compat_path(path);
    match role {
        ApiRole::Operator => {
            is_jobs_or_interrupts
                || (is_audit && *method == axum::http::Method::GET)
                || (is_attempts && *method == axum::http::Method::GET)
                || is_dlq
                || is_a2a_compat
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
    if state.auth.compat_node_secret_hash.is_some() {
        methods.push("bearer(node_secret)");
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
    let method = request.method().clone();
    let path = request.uri().path().to_string();
    let has_runtime_repo_keys = has_runtime_repo_api_keys(&state);
    let compat_node_secret_only = state.auth.compat_node_secret_hash.is_some()
        && state.auth.bearer_token.is_none()
        && state.auth.api_key_hash.is_none()
        && state.auth.keyed_api_keys.is_empty()
        && !has_runtime_repo_keys;
    if compat_node_secret_only && !is_a2a_compat_path(&path) {
        return next.run(request).await;
    }

    let auth_enabled = state.auth.is_enabled() || has_runtime_repo_keys;
    if !auth_enabled {
        return next.run(request).await;
    }

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

#[cfg(feature = "evolution-network-experimental")]
fn validate_sender_id(sender_id: &str) -> Result<(), ApiError> {
    if sender_id.trim().is_empty() {
        return Err(ApiError::bad_request("sender_id must not be empty"));
    }
    Ok(())
}

#[cfg(all(
    feature = "agent-contract-experimental",
    feature = "evolution-network-experimental"
))]
fn resolve_compat_sender_id(
    sender_id: Option<String>,
    node_id: Option<String>,
    rid: &str,
) -> Result<String, ApiError> {
    let sender_id = sender_id
        .or(node_id)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            ApiError::bad_request("sender_id (or node_id) must not be empty")
                .with_request_id(rid.to_string())
                .with_details(serde_json::json!({
                    "a2a_error_code": A2aErrorCode::ValidationFailed,
                    "required_one_of": ["sender_id", "node_id"]
                }))
        })?;
    validate_sender_id(&sender_id).map_err(|e| {
        e.with_request_id(rid.to_string())
            .with_details(serde_json::json!({
                "a2a_error_code": A2aErrorCode::ValidationFailed,
                "field": "sender_id"
            }))
    })?;
    Ok(sender_id)
}

#[cfg(all(
    feature = "agent-contract-experimental",
    feature = "evolution-network-experimental"
))]
fn resolve_compat_protocol_version(
    protocol_version: Option<String>,
    rid: &str,
) -> Result<String, ApiError> {
    let protocol_version = protocol_version
        .map(|version| version.trim().to_string())
        .filter(|version| !version.is_empty())
        .unwrap_or_else(|| crate::agent_contract::A2A_PROTOCOL_VERSION_V1.to_string());
    ensure_task_session_protocol_version(&protocol_version, rid)?;
    Ok(protocol_version)
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

#[cfg(all(
    feature = "agent-contract-experimental",
    feature = "evolution-network-experimental"
))]
fn lifecycle_timestamp_ms(now: chrono::DateTime<Utc>) -> u64 {
    now.timestamp_millis().max(0) as u64
}

#[cfg(all(
    feature = "agent-contract-experimental",
    feature = "evolution-network-experimental"
))]
fn datetime_from_unix_ms(
    ms: u64,
    rid: &str,
    field: &str,
) -> Result<chrono::DateTime<Utc>, ApiError> {
    let value = i64::try_from(ms).map_err(|_| {
        ApiError::bad_request(format!("{field} is out of range")).with_request_id(rid.to_string())
    })?;
    chrono::DateTime::from_timestamp_millis(value).ok_or_else(|| {
        ApiError::bad_request(format!("{field} is not a valid unix-millis timestamp"))
            .with_request_id(rid.to_string())
    })
}

#[cfg(all(
    feature = "agent-contract-experimental",
    feature = "evolution-network-experimental"
))]
async fn append_a2a_task_lifecycle_event(
    state: &ExecutionApiState,
    task_id: &str,
    lifecycle_state: A2aTaskLifecycleState,
    summary: &str,
    error: Option<A2aErrorEnvelope>,
) {
    let event = A2aTaskLifecycleEvent {
        task_id: task_id.to_string(),
        state: lifecycle_state,
        summary: summary.to_string(),
        updated_at_ms: lifecycle_timestamp_ms(Utc::now()),
        error,
    };
    let mut events = state.a2a_task_lifecycle_events.write().await;
    let task_events = events.entry(task_id.to_string()).or_default();
    task_events.push(event);
    if task_events.len() > A2A_TASK_EVENT_HISTORY_LIMIT {
        let overflow = task_events.len() - A2A_TASK_EVENT_HISTORY_LIMIT;
        task_events.drain(..overflow);
    }
}

async fn record_task_queued(state: &ExecutionApiState, task_id: &str, summary: &str) {
    #[cfg(all(
        feature = "agent-contract-experimental",
        feature = "evolution-network-experimental"
    ))]
    {
        append_a2a_task_lifecycle_event(
            state,
            task_id,
            A2aTaskLifecycleState::Queued,
            summary,
            None,
        )
        .await;
        return;
    }

    #[cfg(not(all(
        feature = "agent-contract-experimental",
        feature = "evolution-network-experimental"
    )))]
    let _ = (state, task_id, summary);
}

async fn record_task_running(state: &ExecutionApiState, task_id: &str, summary: &str) {
    #[cfg(all(
        feature = "agent-contract-experimental",
        feature = "evolution-network-experimental"
    ))]
    {
        append_a2a_task_lifecycle_event(
            state,
            task_id,
            A2aTaskLifecycleState::Running,
            summary,
            None,
        )
        .await;
        return;
    }

    #[cfg(not(all(
        feature = "agent-contract-experimental",
        feature = "evolution-network-experimental"
    )))]
    let _ = (state, task_id, summary);
}

async fn record_task_succeeded(state: &ExecutionApiState, task_id: &str, summary: &str) {
    #[cfg(all(
        feature = "agent-contract-experimental",
        feature = "evolution-network-experimental"
    ))]
    {
        append_a2a_task_lifecycle_event(
            state,
            task_id,
            A2aTaskLifecycleState::Succeeded,
            summary,
            None,
        )
        .await;
        return;
    }

    #[cfg(not(all(
        feature = "agent-contract-experimental",
        feature = "evolution-network-experimental"
    )))]
    let _ = (state, task_id, summary);
}

async fn record_task_failed(
    state: &ExecutionApiState,
    task_id: &str,
    summary: &str,
    details: Option<String>,
) {
    #[cfg(all(
        feature = "agent-contract-experimental",
        feature = "evolution-network-experimental"
    ))]
    {
        append_a2a_task_lifecycle_event(
            state,
            task_id,
            A2aTaskLifecycleState::Failed,
            summary,
            Some(A2aErrorEnvelope {
                code: A2aErrorCode::Internal,
                message: summary.to_string(),
                retriable: false,
                details,
            }),
        )
        .await;
        return;
    }

    #[cfg(not(all(
        feature = "agent-contract-experimental",
        feature = "evolution-network-experimental"
    )))]
    let _ = (state, task_id, summary, details);
}

async fn record_task_cancelled(state: &ExecutionApiState, task_id: &str, summary: &str) {
    #[cfg(all(
        feature = "agent-contract-experimental",
        feature = "evolution-network-experimental"
    ))]
    {
        append_a2a_task_lifecycle_event(
            state,
            task_id,
            A2aTaskLifecycleState::Cancelled,
            summary,
            None,
        )
        .await;
        return;
    }

    #[cfg(not(all(
        feature = "agent-contract-experimental",
        feature = "evolution-network-experimental"
    )))]
    let _ = (state, task_id, summary);
}

#[cfg(all(
    feature = "agent-contract-experimental",
    feature = "evolution-network-experimental"
))]
fn ensure_task_session_protocol_version(version: &str, rid: &str) -> Result<(), ApiError> {
    let supported_versions = [
        crate::agent_contract::A2A_PROTOCOL_VERSION_V1,
        A2A_TASK_SESSION_PROTOCOL_VERSION,
    ];
    if supported_versions
        .iter()
        .any(|supported| version == *supported)
    {
        return Ok(());
    }
    Err(
        ApiError::bad_request("incompatible a2a task session protocol version")
            .with_request_id(rid.to_string())
            .with_details(serde_json::json!({
                "a2a_error_code": A2aErrorCode::UnsupportedProtocol,
                "expected": supported_versions,
                "actual": version
            })),
    )
}

#[cfg(all(
    feature = "agent-contract-experimental",
    feature = "evolution-network-experimental"
))]
fn derive_replay_feedback(req: &A2aTaskSessionCompletionRequest) -> ReplayFeedback {
    ReplayFeedback {
        used_capsule: req.used_capsule,
        capsule_id: req.capsule_id.clone(),
        planner_directive: if req.used_capsule {
            ReplayPlannerDirective::SkipPlanner
        } else {
            ReplayPlannerDirective::PlanFallback
        },
        reasoning_steps_avoided: req.reasoning_steps_avoided,
        fallback_reason: req.fallback_reason.clone(),
        task_class_id: req.task_class_id.clone(),
        task_label: req.task_label.clone(),
        summary: req.summary.clone(),
    }
}

#[cfg(all(
    feature = "agent-contract-experimental",
    feature = "evolution-network-experimental"
))]
fn terminal_session_state(terminal_state: &A2aTaskLifecycleState) -> Option<A2aTaskSessionState> {
    match terminal_state {
        A2aTaskLifecycleState::Succeeded => Some(A2aTaskSessionState::Completed),
        A2aTaskLifecycleState::Failed => Some(A2aTaskSessionState::Failed),
        A2aTaskLifecycleState::Cancelled => Some(A2aTaskSessionState::Cancelled),
        A2aTaskLifecycleState::Queued | A2aTaskLifecycleState::Running => None,
    }
}

#[cfg(all(
    feature = "agent-contract-experimental",
    feature = "evolution-network-experimental"
))]
fn task_session_ack(
    session: &A2aTaskSessionSnapshot,
    summary: &str,
    retryable: bool,
    retry_after_ms: Option<u64>,
) -> A2aTaskSessionAck {
    A2aTaskSessionAck {
        session_id: session.session_id.clone(),
        task_id: session.task_id.clone(),
        state: session.state.clone(),
        summary: summary.to_string(),
        retryable,
        retry_after_ms,
        updated_at_ms: session.updated_at_ms,
    }
}

#[cfg(feature = "evolution-network-experimental")]
pub async fn evolution_publish(
    State(state): State<ExecutionApiState>,
    headers: HeaderMap,
    Json(req): Json<PublishRequest>,
) -> Result<Json<ApiEnvelope<ImportOutcome>>, ApiError> {
    let rid = request_id(&headers);
    validate_sender_id(&req.sender_id).map_err(|e| e.with_request_id(rid.clone()))?;
    #[cfg(feature = "agent-contract-experimental")]
    let principal = resolve_a2a_principal(&headers, &state);
    #[cfg(feature = "agent-contract-experimental")]
    ensure_a2a_authorized_action(
        &state,
        &req.sender_id,
        A2aCapability::EvolutionPublish,
        A2aPrivilegeAction::EvolutionPublish,
        principal.as_ref(),
        &rid,
    )
    .await?;
    let outcome = state
        .evolution_node
        .accept_publish_request(&req)
        .map_err(|e| ApiError::internal(e.to_string()).with_request_id(rid.clone()))?;
    Ok(Json(ApiEnvelope {
        meta: ApiMeta::ok(),
        request_id: rid,
        data: outcome,
    }))
}

#[cfg(feature = "evolution-network-experimental")]
pub async fn evolution_fetch(
    State(state): State<ExecutionApiState>,
    headers: HeaderMap,
    Json(req): Json<FetchQuery>,
) -> Result<Json<ApiEnvelope<FetchResponse>>, ApiError> {
    let rid = request_id(&headers);
    validate_sender_id(&req.sender_id).map_err(|e| e.with_request_id(rid.clone()))?;
    #[cfg(feature = "agent-contract-experimental")]
    let principal = resolve_a2a_principal(&headers, &state);
    #[cfg(feature = "agent-contract-experimental")]
    ensure_a2a_authorized_action(
        &state,
        &req.sender_id,
        A2aCapability::EvolutionFetch,
        A2aPrivilegeAction::EvolutionFetch,
        principal.as_ref(),
        &rid,
    )
    .await?;
    let response = state
        .evolution_node
        .fetch_assets("execution-api", &req)
        .map_err(|e| ApiError::internal(e.to_string()).with_request_id(rid.clone()))?;

    Ok(Json(ApiEnvelope {
        meta: ApiMeta::ok(),
        request_id: rid,
        data: response,
    }))
}

#[cfg(feature = "evolution-network-experimental")]
pub async fn evolution_revoke(
    State(state): State<ExecutionApiState>,
    headers: HeaderMap,
    Json(req): Json<RevokeNotice>,
) -> Result<Json<ApiEnvelope<RevokeNotice>>, ApiError> {
    let rid = request_id(&headers);
    validate_sender_id(&req.sender_id).map_err(|e| e.with_request_id(rid.clone()))?;
    #[cfg(feature = "agent-contract-experimental")]
    let principal = resolve_a2a_principal(&headers, &state);
    #[cfg(feature = "agent-contract-experimental")]
    ensure_a2a_authorized_action(
        &state,
        &req.sender_id,
        A2aCapability::EvolutionRevoke,
        A2aPrivilegeAction::EvolutionRevoke,
        principal.as_ref(),
        &rid,
    )
    .await?;
    let response = state
        .evolution_node
        .revoke_assets(&req)
        .map_err(|e| ApiError::internal(e.to_string()).with_request_id(rid.clone()))?;

    Ok(Json(ApiEnvelope {
        meta: ApiMeta::ok(),
        request_id: rid,
        data: response,
    }))
}

#[cfg(all(
    feature = "agent-contract-experimental",
    feature = "evolution-network-experimental"
))]
fn runtime_a2a_capabilities() -> Vec<A2aCapability> {
    vec![
        A2aCapability::Coordination,
        A2aCapability::MutationProposal,
        A2aCapability::ReplayFeedback,
        A2aCapability::SupervisedDevloop,
        A2aCapability::EvolutionPublish,
        A2aCapability::EvolutionFetch,
        A2aCapability::EvolutionRevoke,
    ]
}

#[cfg(all(
    feature = "agent-contract-experimental",
    feature = "evolution-network-experimental"
))]
fn negotiate_a2a_handshake(req: &A2aHandshakeRequest) -> A2aHandshakeResponse {
    let Some(negotiated_protocol) = req.negotiate_supported_protocol() else {
        return A2aHandshakeResponse::reject(
            A2aErrorCode::UnsupportedProtocol,
            "unsupported protocol",
            Some(format!(
                "expected {} in {:?}",
                crate::agent_contract::A2A_PROTOCOL_NAME,
                crate::agent_contract::A2A_SUPPORTED_PROTOCOL_VERSIONS
            )),
        );
    };

    let enabled_capabilities = runtime_a2a_capabilities()
        .into_iter()
        .filter(|capability| req.advertised_capabilities.contains(capability))
        .collect::<Vec<_>>();
    if enabled_capabilities.is_empty() {
        return A2aHandshakeResponse::reject(
            A2aErrorCode::UnsupportedCapability,
            "no overlapping capabilities",
            Some("agent advertised capabilities do not intersect runtime capabilities".into()),
        );
    }

    A2aHandshakeResponse {
        accepted: true,
        negotiated_protocol: Some(negotiated_protocol),
        enabled_capabilities,
        message: Some("handshake accepted".to_string()),
        error: None,
    }
}

#[cfg(all(
    feature = "agent-contract-experimental",
    feature = "evolution-network-experimental"
))]
fn privilege_profile_from_handshake(
    capability_level: &AgentCapabilityLevel,
) -> A2aPrivilegeProfile {
    match capability_level {
        AgentCapabilityLevel::A0 | AgentCapabilityLevel::A1 => A2aPrivilegeProfile::Observer,
        AgentCapabilityLevel::A2 | AgentCapabilityLevel::A3 => A2aPrivilegeProfile::Operator,
        AgentCapabilityLevel::A4 => A2aPrivilegeProfile::Governor,
    }
}

#[cfg(all(
    feature = "agent-contract-experimental",
    feature = "evolution-network-experimental"
))]
fn privilege_profile_from_capabilities(
    enabled_capabilities: &[A2aCapability],
) -> A2aPrivilegeProfile {
    if enabled_capabilities.contains(&A2aCapability::EvolutionRevoke)
        || enabled_capabilities.contains(&A2aCapability::MutationProposal)
    {
        return A2aPrivilegeProfile::Governor;
    }
    if enabled_capabilities.contains(&A2aCapability::EvolutionPublish)
        || enabled_capabilities.contains(&A2aCapability::SupervisedDevloop)
        || enabled_capabilities.contains(&A2aCapability::Coordination)
    {
        return A2aPrivilegeProfile::Operator;
    }
    A2aPrivilegeProfile::Observer
}

#[cfg(all(
    feature = "agent-contract-experimental",
    feature = "evolution-network-experimental"
))]
#[cfg(feature = "sqlite-persistence")]
fn append_a2a_privilege_audit_log(
    state: &ExecutionApiState,
    sender_id: &str,
    capability: &A2aCapability,
    action: A2aPrivilegeAction,
    principal: Option<&A2aSessionPrincipal>,
    profile: Option<&A2aPrivilegeProfile>,
    result: &str,
    reason: &str,
    request_id: &str,
) {
    let Some(repo) = state.runtime_repo.as_ref() else {
        return;
    };
    let entry = AuditLogEntry {
        actor_type: principal
            .map(|p| p.actor_type.clone())
            .unwrap_or_else(|| "anonymous".to_string()),
        actor_id: principal.and_then(|p| p.actor_id.clone()),
        actor_role: principal.map(|p| p.actor_role.clone()),
        action: format!("a2a.privilege.{}", action.as_str()),
        resource_type: "sender_id".to_string(),
        resource_id: Some(sender_id.to_string()),
        result: result.to_string(),
        request_id: request_id.to_string(),
        details_json: serde_json::to_string(&serde_json::json!({
            "sender_id": sender_id,
            "action": action.as_str(),
            "required_capability": format!("{capability:?}"),
            "privilege_profile": profile.map(|item| item.as_str()),
            "reason": reason,
            "principal": principal.map(|p| serde_json::json!({
                "actor_type": p.actor_type.clone(),
                "actor_id": p.actor_id.clone(),
                "actor_role": p.actor_role.clone(),
            })),
        }))
        .ok(),
    };
    let _ = repo.append_audit_log(&entry);
}

#[cfg(all(
    feature = "agent-contract-experimental",
    feature = "evolution-network-experimental"
))]
#[cfg(not(feature = "sqlite-persistence"))]
fn append_a2a_privilege_audit_log(
    _state: &ExecutionApiState,
    _sender_id: &str,
    _capability: &A2aCapability,
    _action: A2aPrivilegeAction,
    _principal: Option<&A2aSessionPrincipal>,
    _profile: Option<&A2aPrivilegeProfile>,
    _result: &str,
    _reason: &str,
    _request_id: &str,
) {
}

#[cfg(all(
    feature = "agent-contract-experimental",
    feature = "evolution-network-experimental"
))]
async fn ensure_a2a_authorized_action(
    state: &ExecutionApiState,
    sender_id: &str,
    capability: A2aCapability,
    action: A2aPrivilegeAction,
    principal: Option<&A2aSessionPrincipal>,
    rid: &str,
) -> Result<(), ApiError> {
    let now = Utc::now();
    #[cfg(feature = "sqlite-persistence")]
    let mut session = state.a2a_sessions.read().await.get(sender_id).cloned();
    #[cfg(not(feature = "sqlite-persistence"))]
    let session = state.a2a_sessions.read().await.get(sender_id).cloned();

    #[cfg(feature = "sqlite-persistence")]
    if session.is_none() {
        if let Some(repo) = state.runtime_repo.as_ref() {
            if let Some(stored) = repo
                .get_active_a2a_session(sender_id, now)
                .map_err(|e| ApiError::internal(e.to_string()).with_request_id(rid.to_string()))?
            {
                let enabled_capabilities: Vec<A2aCapability> =
                    serde_json::from_str(&stored.enabled_capabilities_json).map_err(|e| {
                        ApiError::internal(format!("invalid persisted a2a capabilities: {}", e))
                            .with_request_id(rid.to_string())
                    })?;
                let principal_from_store = match (stored.actor_type, stored.actor_role) {
                    (Some(actor_type), Some(actor_role)) => Some(A2aSessionPrincipal {
                        actor_type,
                        actor_id: stored.actor_id,
                        actor_role,
                    }),
                    (None, None) => None,
                    _ => {
                        return Err(ApiError::internal(
                            "invalid persisted a2a principal binding shape",
                        )
                        .with_request_id(rid.to_string()))
                    }
                };
                let hydrated = A2aSession {
                    negotiated_protocol: A2aProtocol {
                        name: stored.protocol,
                        version: stored.protocol_version,
                    },
                    privilege_profile: privilege_profile_from_capabilities(&enabled_capabilities),
                    enabled_capabilities,
                    principal: principal_from_store,
                    expires_at: stored.expires_at,
                };
                state
                    .a2a_sessions
                    .write()
                    .await
                    .insert(sender_id.to_string(), hydrated.clone());
                session = Some(hydrated);
            }
        }
    }

    let Some(session) = session else {
        append_a2a_privilege_audit_log(
            state,
            sender_id,
            &capability,
            action,
            principal,
            None,
            "denied",
            "missing_handshake",
            rid,
        );
        return Err(
            ApiError::forbidden("a2a handshake required before calling evolution routes")
                .with_request_id(rid.to_string())
                .with_details(serde_json::json!({
                    "sender_id": sender_id,
                    "required_capability": format!("{capability:?}"),
                    "handshake_endpoint": "/v1/evolution/a2a/handshake",
                })),
        );
    };
    if session.expires_at <= now {
        state.a2a_sessions.write().await.remove(sender_id);
        #[cfg(feature = "sqlite-persistence")]
        if let Some(repo) = state.runtime_repo.as_ref() {
            let _ = repo.purge_expired_a2a_sessions(now);
        }
        append_a2a_privilege_audit_log(
            state,
            sender_id,
            &capability,
            action,
            principal,
            Some(&session.privilege_profile),
            "denied",
            "session_expired",
            rid,
        );
        return Err(
            ApiError::forbidden("a2a session expired; handshake required")
                .with_request_id(rid.to_string())
                .with_details(serde_json::json!({
                    "sender_id": sender_id,
                    "handshake_endpoint": "/v1/evolution/a2a/handshake",
                })),
        );
    }
    if session.principal.as_ref() != principal {
        append_a2a_privilege_audit_log(
            state,
            sender_id,
            &capability,
            action,
            principal,
            Some(&session.privilege_profile),
            "denied",
            "principal_mismatch",
            rid,
        );
        return Err(
            ApiError::forbidden("negotiated a2a session principal does not match caller")
                .with_request_id(rid.to_string())
                .with_details(serde_json::json!({
                    "sender_id": sender_id,
                    "expected_principal": session.principal.as_ref().map(|p| serde_json::json!({
                        "actor_type": p.actor_type.clone(),
                        "actor_id": p.actor_id.clone(),
                        "actor_role": p.actor_role.clone(),
                    })),
                    "actual_principal": principal.map(|p| serde_json::json!({
                        "actor_type": p.actor_type.clone(),
                        "actor_id": p.actor_id.clone(),
                        "actor_role": p.actor_role.clone(),
                    })),
                })),
        );
    }
    if !session.enabled_capabilities.contains(&capability) {
        append_a2a_privilege_audit_log(
            state,
            sender_id,
            &capability,
            action,
            principal,
            Some(&session.privilege_profile),
            "denied",
            "missing_capability",
            rid,
        );
        return Err(ApiError::forbidden(
            "negotiated capabilities do not allow this evolution action",
        )
        .with_request_id(rid.to_string())
        .with_details(serde_json::json!({
            "sender_id": sender_id,
            "required_capability": format!("{capability:?}"),
            "negotiated_protocol": format!(
                "{}@{}",
                session.negotiated_protocol.name,
                session.negotiated_protocol.version
            ),
            "enabled_capabilities": session
                .enabled_capabilities
                .iter()
                .map(|item| format!("{item:?}"))
                .collect::<Vec<_>>(),
        })));
    }
    if !session.privilege_profile.allows(action) {
        append_a2a_privilege_audit_log(
            state,
            sender_id,
            &capability,
            action,
            principal,
            Some(&session.privilege_profile),
            "denied",
            "profile_denied",
            rid,
        );
        return Err(
            ApiError::forbidden("a2a privilege profile does not allow this action")
                .with_request_id(rid.to_string())
                .with_details(serde_json::json!({
                    "sender_id": sender_id,
                    "action": action.as_str(),
                    "required_capability": format!("{capability:?}"),
                    "privilege_profile": session.privilege_profile.as_str(),
                })),
        );
    }
    append_a2a_privilege_audit_log(
        state,
        sender_id,
        &capability,
        action,
        principal,
        Some(&session.privilege_profile),
        "allowed",
        "authorized",
        rid,
    );
    Ok(())
}

#[cfg(all(
    feature = "agent-contract-experimental",
    feature = "evolution-network-experimental"
))]
pub async fn evolution_a2a_handshake(
    State(state): State<ExecutionApiState>,
    headers: HeaderMap,
    Json(req): Json<A2aHandshakeRequest>,
) -> Result<Json<ApiEnvelope<A2aHandshakeResponse>>, ApiError> {
    let rid = request_id(&headers);
    validate_sender_id(&req.agent_id).map_err(|e| e.with_request_id(rid.clone()))?;
    let principal = resolve_a2a_principal(&headers, &state);
    let now = Utc::now();
    let response = negotiate_a2a_handshake(&req);
    if response.accepted {
        if let Some(protocol) = response.negotiated_protocol.clone() {
            let expires_at = now + Duration::hours(A2A_SESSION_TTL_HOURS);
            let session = A2aSession {
                negotiated_protocol: protocol.clone(),
                enabled_capabilities: response.enabled_capabilities.clone(),
                privilege_profile: privilege_profile_from_handshake(&req.capability_level),
                principal: principal.clone(),
                expires_at,
            };
            state
                .a2a_sessions
                .write()
                .await
                .insert(req.agent_id.clone(), session.clone());
            #[cfg(feature = "sqlite-persistence")]
            if let Some(repo) = state.runtime_repo.as_ref() {
                repo.upsert_a2a_session(&A2aSessionRow {
                    sender_id: req.agent_id.clone(),
                    protocol: protocol.name,
                    protocol_version: protocol.version,
                    enabled_capabilities_json: serde_json::to_string(&session.enabled_capabilities)
                        .map_err(|e| {
                            ApiError::internal(format!(
                                "serialize negotiated a2a capabilities: {}",
                                e
                            ))
                            .with_request_id(rid.clone())
                        })?,
                    actor_type: session.principal.as_ref().map(|p| p.actor_type.clone()),
                    actor_id: session.principal.as_ref().and_then(|p| p.actor_id.clone()),
                    actor_role: session.principal.as_ref().map(|p| p.actor_role.clone()),
                    negotiated_at: now,
                    expires_at: session.expires_at,
                    updated_at: now,
                })
                .map_err(|e| ApiError::internal(e.to_string()).with_request_id(rid.clone()))?;
            }
        }
    }
    Ok(Json(ApiEnvelope {
        meta: ApiMeta::ok(),
        request_id: rid,
        data: response,
    }))
}

#[cfg(all(
    feature = "agent-contract-experimental",
    feature = "evolution-network-experimental"
))]
pub async fn evolution_a2a_hello(
    state: State<ExecutionApiState>,
    headers: HeaderMap,
    req: Json<A2aHandshakeRequest>,
) -> Result<Json<ApiEnvelope<A2aHandshakeResponse>>, ApiError> {
    evolution_a2a_handshake(state, headers, req).await
}

#[cfg(all(
    feature = "agent-contract-experimental",
    feature = "evolution-network-experimental"
))]
pub async fn evolution_a2a_hello_compat(
    state: State<ExecutionApiState>,
    headers: HeaderMap,
    Json(raw): Json<Value>,
) -> Result<Json<ApiEnvelope<A2aHandshakeResponse>>, ApiError> {
    let rid = request_id(&headers);
    let req = parse_gep_hello_or_plain(raw, &rid)?;
    evolution_a2a_hello(state, headers, Json(req)).await
}

#[cfg(all(
    feature = "agent-contract-experimental",
    feature = "evolution-network-experimental"
))]
pub async fn evolution_a2a_fetch_compat(
    state: State<ExecutionApiState>,
    headers: HeaderMap,
    Json(raw): Json<Value>,
) -> Result<Json<ApiEnvelope<A2aCompatFetchResponse>>, ApiError> {
    let rid = request_id(&headers);
    let req = parse_gep_envelope_or_plain(raw, Some("fetch"), &rid)?;
    state.runtime_metrics.record_a2a_fetch();
    evolution_a2a_fetch(state, headers, Json(req)).await
}

#[cfg(all(
    feature = "agent-contract-experimental",
    feature = "evolution-network-experimental"
))]
pub async fn evolution_a2a_tasks_distribute_compat(
    state: State<ExecutionApiState>,
    headers: HeaderMap,
    Json(raw): Json<Value>,
) -> Result<Json<ApiEnvelope<A2aCompatDistributeResponse>>, ApiError> {
    let rid = request_id(&headers);
    let req = parse_gep_envelope_or_plain(raw, None, &rid)?;
    evolution_a2a_tasks_distribute(state, headers, Json(req)).await
}

#[cfg(all(
    feature = "agent-contract-experimental",
    feature = "evolution-network-experimental"
))]
pub async fn evolution_a2a_tasks_claim_compat(
    state: State<ExecutionApiState>,
    headers: HeaderMap,
    Json(raw): Json<Value>,
) -> Result<Json<ApiEnvelope<A2aCompatClaimResponse>>, ApiError> {
    let rid = request_id(&headers);
    let req = parse_gep_envelope_or_plain(raw, None, &rid)?;
    state.runtime_metrics.record_a2a_task_claim();
    evolution_a2a_tasks_claim(state, headers, Json(req)).await
}

#[cfg(all(
    feature = "agent-contract-experimental",
    feature = "evolution-network-experimental"
))]
pub async fn evolution_a2a_tasks_report_compat(
    state: State<ExecutionApiState>,
    headers: HeaderMap,
    Json(raw): Json<Value>,
) -> Result<Json<ApiEnvelope<A2aCompatReportResponse>>, ApiError> {
    let rid = request_id(&headers);
    let req = parse_gep_envelope_or_plain(raw, None, &rid)?;
    evolution_a2a_tasks_report(state, headers, Json(req)).await
}

#[cfg(all(
    feature = "agent-contract-experimental",
    feature = "evolution-network-experimental"
))]
pub async fn evolution_a2a_task_complete_compat(
    state: State<ExecutionApiState>,
    headers: HeaderMap,
    Json(raw): Json<Value>,
) -> Result<Json<ApiEnvelope<A2aCompatReportResponse>>, ApiError> {
    let rid = request_id(&headers);
    let req = parse_gep_envelope_or_plain(raw, None, &rid)?;
    state.runtime_metrics.record_a2a_task_complete();
    evolution_a2a_task_complete(state, headers, Json(req)).await
}

#[cfg(all(
    feature = "agent-contract-experimental",
    feature = "evolution-network-experimental"
))]
pub async fn evolution_a2a_work_claim_compat(
    state: State<ExecutionApiState>,
    headers: HeaderMap,
    Json(raw): Json<Value>,
) -> Result<Json<ApiEnvelope<A2aCompatWorkClaimResponse>>, ApiError> {
    let rid = request_id(&headers);
    let req = parse_gep_envelope_or_plain(raw, None, &rid)?;
    state.runtime_metrics.record_a2a_work_claim();
    evolution_a2a_work_claim(state, headers, Json(req)).await
}

#[cfg(all(
    feature = "agent-contract-experimental",
    feature = "evolution-network-experimental"
))]
pub async fn evolution_a2a_work_complete_compat(
    state: State<ExecutionApiState>,
    headers: HeaderMap,
    Json(raw): Json<Value>,
) -> Result<Json<ApiEnvelope<A2aCompatWorkCompleteResponse>>, ApiError> {
    let rid = request_id(&headers);
    let req = parse_gep_envelope_or_plain(raw, None, &rid)?;
    state.runtime_metrics.record_a2a_work_complete();
    evolution_a2a_work_complete(state, headers, Json(req)).await
}

#[cfg(all(
    feature = "agent-contract-experimental",
    feature = "evolution-network-experimental"
))]
pub async fn evolution_a2a_heartbeat_compat(
    state: State<ExecutionApiState>,
    headers: HeaderMap,
    Json(raw): Json<Value>,
) -> Result<Json<ApiEnvelope<A2aCompatHeartbeatResponse>>, ApiError> {
    let rid = request_id(&headers);
    let req = parse_gep_envelope_or_plain(raw, None, &rid)?;
    state.runtime_metrics.record_a2a_heartbeat();
    evolution_a2a_heartbeat(state, headers, Json(req)).await
}

#[cfg(all(
    feature = "agent-contract-experimental",
    feature = "evolution-network-experimental"
))]
fn build_a2a_fetch_task(
    session_id: String,
    task_id: String,
    task_summary: String,
    dispatch_id: String,
    claimed_by: Option<&str>,
    lease_expires_at_ms: Option<u64>,
    now_ms: u64,
) -> A2aCompatFetchTask {
    let claimable =
        claimed_by.is_none() || lease_expires_at_ms.map(|ms| ms <= now_ms).unwrap_or(true);
    A2aCompatFetchTask {
        session_id,
        task_id,
        task_summary,
        dispatch_id,
        claimable,
        lease_expires_at_ms,
    }
}

#[cfg(all(
    feature = "agent-contract-experimental",
    feature = "evolution-network-experimental"
))]
async fn list_a2a_fetch_tasks(
    state: &ExecutionApiState,
    sender_id: &str,
    protocol_version: &str,
    rid: &str,
) -> Result<Vec<A2aCompatFetchTask>, ApiError> {
    let now_ms = lifecycle_timestamp_ms(Utc::now());
    #[cfg(not(feature = "sqlite-persistence"))]
    let _ = rid;

    #[cfg(feature = "sqlite-persistence")]
    if let Some(repo) = state.runtime_repo.as_ref() {
        let tasks = repo
            .list_a2a_compat_tasks(sender_id, protocol_version)
            .map_err(|e| ApiError::internal(e.to_string()).with_request_id(rid.to_string()))?;
        return Ok(tasks
            .into_iter()
            .map(|task| {
                let lease_expires_at_ms = task
                    .lease_expires_at
                    .map(|expires_at| expires_at.timestamp_millis().max(0) as u64);
                build_a2a_fetch_task(
                    task.session_id,
                    task.task_id,
                    task.task_summary,
                    task.dispatch_id,
                    task.claimed_by_sender_id.as_deref(),
                    lease_expires_at_ms,
                    now_ms,
                )
            })
            .collect());
    }

    let queue = state.a2a_compat_task_queue.read().await;
    Ok(queue
        .iter()
        .filter(|entry| {
            entry.owner_sender_id == sender_id && entry.protocol_version == protocol_version
        })
        .map(|entry| {
            build_a2a_fetch_task(
                entry.session_id.clone(),
                entry.task_id.clone(),
                entry.task_summary.clone(),
                entry.dispatch_id.clone(),
                entry.claimed_by.as_deref(),
                entry.lease_expires_at_ms,
                now_ms,
            )
        })
        .collect())
}

#[cfg(all(
    feature = "agent-contract-experimental",
    feature = "evolution-network-experimental"
))]
pub async fn evolution_a2a_fetch(
    State(state): State<ExecutionApiState>,
    headers: HeaderMap,
    Json(req): Json<A2aCompatFetchRequest>,
) -> Result<Json<ApiEnvelope<A2aCompatFetchResponse>>, ApiError> {
    let rid = request_id(&headers);
    let sender_id = resolve_compat_sender_id(req.sender_id.clone(), req.node_id.clone(), &rid)?;
    let protocol_version = resolve_compat_protocol_version(req.protocol_version.clone(), &rid)?;

    let principal = resolve_a2a_principal(&headers, &state);
    ensure_a2a_authorized_action(
        &state,
        &sender_id,
        A2aCapability::EvolutionFetch,
        A2aPrivilegeAction::EvolutionFetch,
        principal.as_ref(),
        &rid,
    )
    .await?;

    let fetch = state
        .evolution_node
        .fetch_assets(
            "execution-api",
            &FetchQuery {
                sender_id: sender_id.clone(),
                signals: req.signals,
            },
        )
        .map_err(|e| ApiError::internal(e.to_string()).with_request_id(rid.clone()))?;

    let tasks = if req.include_tasks {
        Some(list_a2a_fetch_tasks(&state, &sender_id, &protocol_version, &rid).await?)
    } else {
        None
    };

    Ok(Json(ApiEnvelope {
        meta: ApiMeta::ok(),
        request_id: rid,
        data: A2aCompatFetchResponse {
            sender_id: fetch.sender_id,
            assets: fetch.assets,
            tasks,
        },
    }))
}

#[cfg(all(
    feature = "agent-contract-experimental",
    feature = "evolution-network-experimental"
))]
async fn find_a2a_session_id_for_task(
    state: &ExecutionApiState,
    sender_id: &str,
    task_id: &str,
) -> Option<String> {
    state
        .a2a_task_sessions
        .read()
        .await
        .iter()
        .find_map(|(session_id, session)| {
            if session.sender_id == sender_id && session.task_id == task_id {
                Some(session_id.clone())
            } else {
                None
            }
        })
}

#[cfg(all(
    feature = "agent-contract-experimental",
    feature = "evolution-network-experimental"
))]
fn resolve_compat_task_complete_status(
    req: &A2aCompatTaskCompleteRequest,
) -> A2aCompatReportStatus {
    if let Some(status) = req.status.clone() {
        return status;
    }
    match req.success {
        Some(false) => A2aCompatReportStatus::Failed,
        Some(true) | None => A2aCompatReportStatus::Succeeded,
    }
}

#[cfg(all(
    feature = "agent-contract-experimental",
    feature = "evolution-network-experimental"
))]
fn default_task_complete_summary(status: &A2aCompatReportStatus) -> &'static str {
    match status {
        A2aCompatReportStatus::Succeeded => "a2a task completed",
        A2aCompatReportStatus::Failed => "a2a task failed",
        A2aCompatReportStatus::Cancelled => "a2a task cancelled",
        A2aCompatReportStatus::Running => "a2a task in progress",
    }
}

#[cfg(all(
    feature = "agent-contract-experimental",
    feature = "evolution-network-experimental"
))]
pub async fn evolution_a2a_task_complete(
    state: State<ExecutionApiState>,
    headers: HeaderMap,
    Json(req): Json<A2aCompatTaskCompleteRequest>,
) -> Result<Json<ApiEnvelope<A2aCompatReportResponse>>, ApiError> {
    let status = resolve_compat_task_complete_status(&req);
    let summary = req
        .summary
        .clone()
        .unwrap_or_else(|| default_task_complete_summary(&status).to_string());
    evolution_a2a_tasks_report(
        state,
        headers,
        Json(A2aCompatReportRequest {
            sender_id: req.sender_id,
            node_id: req.node_id,
            protocol_version: req.protocol_version,
            task_id: req.task_id,
            status,
            summary,
            progress_pct: None,
            retryable: req.retryable,
            retry_after_ms: req.retry_after_ms,
            failure_code: req.failure_code,
            failure_details: req.failure_details,
            used_capsule: req.used_capsule,
            capsule_id: req.capsule_id,
            reasoning_steps_avoided: req.reasoning_steps_avoided,
            fallback_reason: req.fallback_reason,
            task_class_id: req.task_class_id,
            task_label: req.task_label,
        }),
    )
    .await
}

#[cfg(all(
    feature = "agent-contract-experimental",
    feature = "evolution-network-experimental"
))]
async fn resolve_active_work_assignment_task_id(
    state: &ExecutionApiState,
    assignment_id: &str,
    sender_id: &str,
    protocol_version: &str,
    rid: &str,
) -> Result<String, ApiError> {
    #[cfg(not(feature = "sqlite-persistence"))]
    let _ = rid;

    #[cfg(feature = "sqlite-persistence")]
    if let Some(repo) = state.runtime_repo.as_ref() {
        let task = repo
            .get_a2a_compat_task(assignment_id)
            .map_err(|e| ApiError::internal(e.to_string()).with_request_id(rid.to_string()))?
            .ok_or_else(|| {
                ApiError::conflict("a2a work assignment is not active")
                    .with_request_id(rid.to_string())
                    .with_details(serde_json::json!({
                        "assignment_id": assignment_id,
                        "reason": "already_completed_or_unknown"
                    }))
            })?;
        if task.sender_id != sender_id {
            return Err(ApiError::forbidden("a2a work assignment sender mismatch")
                .with_request_id(rid.to_string())
                .with_details(serde_json::json!({
                    "assignment_id": assignment_id,
                    "expected_sender_id": task.sender_id,
                    "actual_sender_id": sender_id
                })));
        }
        if task.protocol_version != protocol_version {
            return Err(
                ApiError::bad_request("a2a work assignment protocol version mismatch")
                    .with_request_id(rid.to_string())
                    .with_details(serde_json::json!({
                        "assignment_id": assignment_id,
                        "expected_protocol_version": task.protocol_version,
                        "actual_protocol_version": protocol_version
                    })),
            );
        }
        match task.claimed_by_sender_id.as_deref() {
            Some(claimed_by) if claimed_by == sender_id => {}
            Some(claimed_by) => {
                return Err(
                    ApiError::forbidden("a2a work assignment is owned by another claimer")
                        .with_request_id(rid.to_string())
                        .with_details(serde_json::json!({
                            "assignment_id": assignment_id,
                            "claimed_by": claimed_by,
                            "reporter": sender_id
                        })),
                );
            }
            None => {
                return Err(
                    ApiError::conflict("a2a work assignment has not been claimed")
                        .with_request_id(rid.to_string())
                        .with_details(serde_json::json!({
                            "assignment_id": assignment_id
                        })),
                );
            }
        }
        if task
            .lease_expires_at
            .map(|expires_at| expires_at <= Utc::now())
            .unwrap_or(false)
        {
            return Err(ApiError::conflict("a2a work assignment lease expired")
                .with_request_id(rid.to_string())
                .with_details(serde_json::json!({
                    "assignment_id": assignment_id
                })));
        }
        return Ok(task.task_id);
    }

    let now_ms = lifecycle_timestamp_ms(Utc::now());
    let queue = state.a2a_compat_task_queue.read().await;
    let entry = queue
        .iter()
        .find(|entry| entry.session_id == assignment_id)
        .ok_or_else(|| {
            ApiError::conflict("a2a work assignment is not active")
                .with_request_id(rid.to_string())
                .with_details(serde_json::json!({
                    "assignment_id": assignment_id,
                    "reason": "already_completed_or_unknown"
                }))
        })?;
    if entry.owner_sender_id != sender_id {
        return Err(ApiError::forbidden("a2a work assignment sender mismatch")
            .with_request_id(rid.to_string())
            .with_details(serde_json::json!({
                "assignment_id": assignment_id,
                "expected_sender_id": entry.owner_sender_id,
                "actual_sender_id": sender_id
            })));
    }
    if entry.protocol_version != protocol_version {
        return Err(
            ApiError::bad_request("a2a work assignment protocol version mismatch")
                .with_request_id(rid.to_string())
                .with_details(serde_json::json!({
                    "assignment_id": assignment_id,
                    "expected_protocol_version": entry.protocol_version,
                    "actual_protocol_version": protocol_version
                })),
        );
    }
    match entry.claimed_by.as_deref() {
        Some(claimed_by) if claimed_by == sender_id => {}
        Some(claimed_by) => {
            return Err(
                ApiError::forbidden("a2a work assignment is owned by another claimer")
                    .with_request_id(rid.to_string())
                    .with_details(serde_json::json!({
                        "assignment_id": assignment_id,
                        "claimed_by": claimed_by,
                        "reporter": sender_id
                    })),
            );
        }
        None => {
            return Err(
                ApiError::conflict("a2a work assignment has not been claimed")
                    .with_request_id(rid.to_string())
                    .with_details(serde_json::json!({
                        "assignment_id": assignment_id
                    })),
            );
        }
    }
    if entry
        .lease_expires_at_ms
        .map(|expires_at_ms| expires_at_ms <= now_ms)
        .unwrap_or(false)
    {
        return Err(ApiError::conflict("a2a work assignment lease expired")
            .with_request_id(rid.to_string())
            .with_details(serde_json::json!({
                "assignment_id": assignment_id
            })));
    }
    Ok(entry.task_id.clone())
}

#[cfg(all(
    feature = "agent-contract-experimental",
    feature = "evolution-network-experimental"
))]
pub async fn evolution_a2a_work_claim(
    state: State<ExecutionApiState>,
    headers: HeaderMap,
    Json(req): Json<A2aCompatWorkClaimRequest>,
) -> Result<Json<ApiEnvelope<A2aCompatWorkClaimResponse>>, ApiError> {
    let claim = evolution_a2a_tasks_claim(
        state,
        headers,
        Json(A2aCompatClaimRequest {
            sender_id: req.sender_id.or(req.worker_id),
            node_id: req.node_id,
            protocol_version: req.protocol_version,
        }),
    )
    .await?;
    let envelope = claim.0;
    let data = envelope.data;
    Ok(Json(ApiEnvelope {
        meta: envelope.meta,
        request_id: envelope.request_id,
        data: A2aCompatWorkClaimResponse {
            claimed: data.claimed,
            assignment: data.task.map(|task| A2aCompatWorkAssignment {
                assignment_id: task.session_id,
                task_id: task.task_id,
                task_summary: task.task_summary,
                dispatch_id: task.dispatch_id,
            }),
            retry_after_ms: data.retry_after_ms,
        },
    }))
}

#[cfg(all(
    feature = "agent-contract-experimental",
    feature = "evolution-network-experimental"
))]
pub async fn evolution_a2a_work_complete(
    State(state): State<ExecutionApiState>,
    headers: HeaderMap,
    Json(req): Json<A2aCompatWorkCompleteRequest>,
) -> Result<Json<ApiEnvelope<A2aCompatWorkCompleteResponse>>, ApiError> {
    let rid = request_id(&headers);
    validate_thread_id(&req.assignment_id).map_err(|e| e.with_request_id(rid.clone()))?;
    let sender_id = resolve_compat_sender_id(
        req.sender_id.clone().or(req.worker_id.clone()),
        req.node_id.clone(),
        &rid,
    )?;
    let protocol_version = resolve_compat_protocol_version(req.protocol_version.clone(), &rid)?;
    let task_id = resolve_active_work_assignment_task_id(
        &state,
        &req.assignment_id,
        &sender_id,
        &protocol_version,
        &rid,
    )
    .await?;
    if let Some(explicit_task_id) = req.task_id.as_deref() {
        validate_thread_id(explicit_task_id).map_err(|e| e.with_request_id(rid.clone()))?;
        if explicit_task_id != task_id {
            return Err(
                ApiError::bad_request("task_id does not match assignment_id")
                    .with_request_id(rid.clone())
                    .with_details(serde_json::json!({
                        "assignment_id": req.assignment_id,
                        "expected_task_id": task_id,
                        "actual_task_id": explicit_task_id
                    })),
            );
        }
    }
    let assignment_id = req.assignment_id.clone();
    let completion = evolution_a2a_task_complete(
        State(state),
        headers,
        Json(A2aCompatTaskCompleteRequest {
            sender_id: Some(sender_id),
            node_id: None,
            protocol_version: Some(protocol_version),
            task_id,
            status: req.status,
            success: req.success,
            summary: req.summary,
            retryable: req.retryable,
            retry_after_ms: req.retry_after_ms,
            failure_code: req.failure_code,
            failure_details: req.failure_details,
            used_capsule: req.used_capsule,
            capsule_id: req.capsule_id,
            reasoning_steps_avoided: req.reasoning_steps_avoided,
            fallback_reason: req.fallback_reason,
            task_class_id: req.task_class_id,
            task_label: req.task_label,
        }),
    )
    .await?;
    let envelope = completion.0;
    let data = envelope.data;
    Ok(Json(ApiEnvelope {
        meta: envelope.meta,
        request_id: envelope.request_id,
        data: A2aCompatWorkCompleteResponse {
            assignment_id,
            task_id: data.task_id,
            state: data.state,
            terminal_state: data.terminal_state,
            summary: data.summary,
        },
    }))
}

#[cfg(all(
    feature = "agent-contract-experimental",
    feature = "evolution-network-experimental"
))]
pub async fn evolution_a2a_heartbeat(
    State(state): State<ExecutionApiState>,
    headers: HeaderMap,
    Json(req): Json<A2aCompatHeartbeatRequest>,
) -> Result<Json<ApiEnvelope<A2aCompatHeartbeatResponse>>, ApiError> {
    let rid = request_id(&headers);
    let sender_id = resolve_compat_sender_id(
        req.sender_id.clone().or(req.worker_id.clone()),
        req.node_id.clone(),
        &rid,
    )?;
    let protocol_version = resolve_compat_protocol_version(req.protocol_version.clone(), &rid)?;
    let principal = resolve_a2a_principal(&headers, &state);
    ensure_a2a_authorized_action(
        &state,
        &sender_id,
        A2aCapability::Coordination,
        A2aPrivilegeAction::TaskSessionStart,
        principal.as_ref(),
        &rid,
    )
    .await?;

    let available_work = list_a2a_fetch_tasks(&state, &sender_id, &protocol_version, &rid)
        .await?
        .into_iter()
        .filter(|task| task.claimable)
        .map(|task| A2aCompatAvailableWorkItem {
            assignment_id: task.session_id,
            task_id: task.task_id,
            task_summary: task.task_summary,
            dispatch_id: task.dispatch_id,
        })
        .collect::<Vec<_>>();
    let available_work_count = available_work.len();

    Ok(Json(ApiEnvelope {
        meta: ApiMeta::ok(),
        request_id: rid,
        data: A2aCompatHeartbeatResponse {
            acknowledged: true,
            worker_id: sender_id,
            available_work_count,
            available_work,
            metadata_accepted: req.metadata.is_some(),
        },
    }))
}

#[cfg(all(
    feature = "agent-contract-experimental",
    feature = "evolution-network-experimental"
))]
pub async fn evolution_a2a_tasks_distribute(
    State(state): State<ExecutionApiState>,
    headers: HeaderMap,
    Json(req): Json<A2aCompatDistributeRequest>,
) -> Result<Json<ApiEnvelope<A2aCompatDistributeResponse>>, ApiError> {
    let rid = request_id(&headers);
    let owner_sender_id =
        resolve_compat_sender_id(req.sender_id.clone(), req.node_id.clone(), &rid)?;
    let protocol_version = resolve_compat_protocol_version(req.protocol_version.clone(), &rid)?;
    validate_thread_id(&req.task_id).map_err(|e| {
        e.with_request_id(rid.clone())
            .with_details(serde_json::json!({
                "a2a_error_code": A2aErrorCode::ValidationFailed,
                "field": "task_id"
            }))
    })?;
    let task_id = req.task_id.clone();
    let task_summary = req.task_summary.clone();
    let dispatch_id = req
        .dispatch_id
        .clone()
        .unwrap_or_else(|| format!("dispatch-{}", req.task_id));
    let dispatch_summary = req
        .summary
        .clone()
        .unwrap_or_else(|| req.task_summary.clone());

    let start = evolution_a2a_session_start(
        State(state.clone()),
        headers.clone(),
        Json(A2aTaskSessionStartRequest {
            sender_id: owner_sender_id.clone(),
            protocol_version: protocol_version.clone(),
            task_id: req.task_id.clone(),
            task_summary: req.task_summary.clone(),
        }),
    )
    .await?;
    let session_id = start.0.data.session_id.clone();

    let dispatch = evolution_a2a_session_dispatch(
        State(state.clone()),
        Path(session_id.clone()),
        headers,
        Json(A2aTaskSessionDispatchRequest {
            sender_id: owner_sender_id.clone(),
            protocol_version: protocol_version.clone(),
            dispatch_id: dispatch_id.clone(),
            summary: dispatch_summary,
        }),
    )
    .await?;
    let envelope = dispatch.0;
    let data = envelope.data;

    #[cfg(feature = "sqlite-persistence")]
    if let Some(repo) = state.runtime_repo.as_ref() {
        let now = Utc::now();
        repo.upsert_a2a_compat_task(&A2aCompatTaskRow {
            session_id: session_id.clone(),
            sender_id: owner_sender_id.clone(),
            protocol_version: protocol_version.clone(),
            task_id: task_id.clone(),
            task_summary: task_summary.clone(),
            dispatch_id: dispatch_id.clone(),
            claimed_by_sender_id: None,
            lease_expires_at: None,
            enqueued_at: now,
            updated_at: now,
        })
        .map_err(|e| {
            ApiError::internal(e.to_string()).with_request_id(envelope.request_id.clone())
        })?;
    } else {
        let mut queue = state.a2a_compat_task_queue.write().await;
        if !queue.iter().any(|entry| entry.session_id == session_id) {
            queue.push_back(A2aCompatQueueEntry {
                session_id: session_id.clone(),
                owner_sender_id,
                protocol_version,
                task_id: task_id.clone(),
                task_summary,
                dispatch_id,
                claimed_by: None,
                lease_expires_at_ms: None,
            });
        }
    }
    #[cfg(not(feature = "sqlite-persistence"))]
    {
        let mut queue = state.a2a_compat_task_queue.write().await;
        if !queue.iter().any(|entry| entry.session_id == session_id) {
            queue.push_back(A2aCompatQueueEntry {
                session_id: session_id.clone(),
                owner_sender_id,
                protocol_version,
                task_id: task_id.clone(),
                task_summary,
                dispatch_id,
                claimed_by: None,
                lease_expires_at_ms: None,
            });
        }
    }

    Ok(Json(ApiEnvelope {
        meta: ApiMeta::ok(),
        request_id: envelope.request_id,
        data: A2aCompatDistributeResponse {
            session_id,
            task_id: data.task_id,
            state: data.state,
            summary: data.summary,
        },
    }))
}

#[cfg(all(
    feature = "agent-contract-experimental",
    feature = "evolution-network-experimental"
))]
pub async fn evolution_a2a_tasks_claim(
    State(state): State<ExecutionApiState>,
    headers: HeaderMap,
    Json(req): Json<A2aCompatClaimRequest>,
) -> Result<Json<ApiEnvelope<A2aCompatClaimResponse>>, ApiError> {
    let rid = request_id(&headers);
    let sender_id = resolve_compat_sender_id(req.sender_id.clone(), req.node_id.clone(), &rid)?;
    let protocol_version = resolve_compat_protocol_version(req.protocol_version.clone(), &rid)?;

    let principal = resolve_a2a_principal(&headers, &state);
    ensure_a2a_authorized_action(
        &state,
        &sender_id,
        A2aCapability::Coordination,
        A2aPrivilegeAction::TaskSessionStart,
        principal.as_ref(),
        &rid,
    )
    .await?;
    let claim_started_at = Instant::now();

    #[cfg(feature = "sqlite-persistence")]
    if let Some(repo) = state.runtime_repo.as_ref() {
        let claim = repo
            .claim_a2a_compat_task(
                &sender_id,
                &protocol_version,
                Utc::now(),
                A2A_COMPAT_CLAIM_LEASE_MS,
            )
            .map_err(|e| ApiError::internal(e.to_string()).with_request_id(rid.clone()))?;
        if claim.reclaimed_expired_lease {
            state.runtime_metrics.record_a2a_task_lease_expired();
        }
        state
            .runtime_metrics
            .record_a2a_task_claim_latency_ms(claim_started_at.elapsed().as_secs_f64() * 1000.0);
        return Ok(Json(ApiEnvelope {
            meta: ApiMeta::ok(),
            request_id: rid,
            data: A2aCompatClaimResponse {
                claimed: claim.task.is_some(),
                task: claim.task.map(|task| A2aCompatClaimTask {
                    session_id: task.session_id,
                    task_id: task.task_id,
                    task_summary: task.task_summary,
                    dispatch_id: task.dispatch_id,
                }),
                retry_after_ms: claim.retry_after_ms,
            },
        }));
    }

    let now_ms = lifecycle_timestamp_ms(Utc::now());
    let mut queue = state.a2a_compat_task_queue.write().await;
    let mut retry_after_ms: Option<u64> = None;

    for entry in queue.iter_mut() {
        if entry.owner_sender_id != sender_id {
            continue;
        }
        if entry.protocol_version != protocol_version {
            continue;
        }
        if let Some(expires_at_ms) = entry.lease_expires_at_ms {
            if expires_at_ms > now_ms {
                let remaining = expires_at_ms.saturating_sub(now_ms);
                retry_after_ms = Some(match retry_after_ms {
                    Some(current_min) => current_min.min(remaining),
                    None => remaining,
                });
                continue;
            }
        }

        let reclaimed_expired_lease = entry.claimed_by.is_some()
            && entry
                .lease_expires_at_ms
                .map(|expires_at_ms| expires_at_ms <= now_ms)
                .unwrap_or(true);
        entry.claimed_by = Some(sender_id.clone());
        entry.lease_expires_at_ms = Some(now_ms + A2A_COMPAT_CLAIM_LEASE_MS);
        if reclaimed_expired_lease {
            state.runtime_metrics.record_a2a_task_lease_expired();
        }
        state
            .runtime_metrics
            .record_a2a_task_claim_latency_ms(claim_started_at.elapsed().as_secs_f64() * 1000.0);
        return Ok(Json(ApiEnvelope {
            meta: ApiMeta::ok(),
            request_id: rid,
            data: A2aCompatClaimResponse {
                claimed: true,
                task: Some(A2aCompatClaimTask {
                    session_id: entry.session_id.clone(),
                    task_id: entry.task_id.clone(),
                    task_summary: entry.task_summary.clone(),
                    dispatch_id: entry.dispatch_id.clone(),
                }),
                retry_after_ms: None,
            },
        }));
    }

    state
        .runtime_metrics
        .record_a2a_task_claim_latency_ms(claim_started_at.elapsed().as_secs_f64() * 1000.0);
    Ok(Json(ApiEnvelope {
        meta: ApiMeta::ok(),
        request_id: rid,
        data: A2aCompatClaimResponse {
            claimed: false,
            task: None,
            retry_after_ms,
        },
    }))
}

#[cfg(all(
    feature = "agent-contract-experimental",
    feature = "evolution-network-experimental"
))]
pub async fn evolution_a2a_tasks_report(
    State(state): State<ExecutionApiState>,
    headers: HeaderMap,
    Json(req): Json<A2aCompatReportRequest>,
) -> Result<Json<ApiEnvelope<A2aCompatReportResponse>>, ApiError> {
    let rid = request_id(&headers);
    let sender_id = resolve_compat_sender_id(req.sender_id.clone(), req.node_id.clone(), &rid)?;
    let protocol_version = resolve_compat_protocol_version(req.protocol_version.clone(), &rid)?;
    let task_id = req.task_id.clone();
    validate_thread_id(&task_id).map_err(|e| {
        e.with_request_id(rid.clone())
            .with_details(serde_json::json!({
                "a2a_error_code": A2aErrorCode::ValidationFailed,
                "field": "task_id"
            }))
    })?;

    let session_id = find_a2a_session_id_for_task(&state, &sender_id, &task_id)
        .await
        .ok_or_else(|| {
            ApiError::not_found(format!(
                "a2a task session not found for sender '{}' and task '{}'",
                sender_id, task_id
            ))
            .with_request_id(rid.clone())
        })?;
    let report_started_at = Instant::now();

    match req.status {
        A2aCompatReportStatus::Running => {
            #[cfg(feature = "sqlite-persistence")]
            if let Some(repo) = state.runtime_repo.as_ref() {
                if let Some(task) = repo
                    .get_a2a_compat_task(&session_id)
                    .map_err(|e| ApiError::internal(e.to_string()).with_request_id(rid.clone()))?
                {
                    if let (Some(claimed_by), Some(lease_expires_at)) =
                        (task.claimed_by_sender_id.as_deref(), task.lease_expires_at)
                    {
                        if claimed_by != sender_id && lease_expires_at > Utc::now() {
                            return Err(ApiError::forbidden(
                                "compat task lease is owned by another claimer",
                            )
                            .with_request_id(rid.clone())
                            .with_details(serde_json::json!({
                                "session_id": session_id.clone(),
                                "task_id": task_id.clone(),
                                "claimed_by": claimed_by,
                                "reporter": sender_id.clone()
                            })));
                        }
                    }
                }
                let touched = repo
                    .touch_a2a_compat_task_lease(
                        &session_id,
                        &sender_id,
                        Utc::now(),
                        A2A_COMPAT_CLAIM_LEASE_MS,
                    )
                    .map_err(|e| ApiError::internal(e.to_string()).with_request_id(rid.clone()))?;
                if !touched {
                    return Err(ApiError::not_found(format!(
                        "a2a compat task lease not found for session '{}'",
                        session_id
                    ))
                    .with_request_id(rid.clone()));
                }
            } else {
                let now_ms = lifecycle_timestamp_ms(Utc::now());
                let mut queue = state.a2a_compat_task_queue.write().await;
                if let Some(entry) = queue
                    .iter_mut()
                    .find(|entry| entry.session_id == session_id)
                {
                    if let Some(claimed_by) = entry.claimed_by.as_deref() {
                        if claimed_by != sender_id {
                            return Err(ApiError::forbidden(
                                "compat task lease is owned by another claimer",
                            )
                            .with_request_id(rid.clone())
                            .with_details(serde_json::json!({
                                "session_id": session_id.clone(),
                                "task_id": task_id.clone(),
                                "claimed_by": claimed_by,
                                "reporter": sender_id.clone()
                            })));
                        }
                    }
                    entry.claimed_by = Some(sender_id.clone());
                    entry.lease_expires_at_ms = Some(now_ms + A2A_COMPAT_CLAIM_LEASE_MS);
                }
            }
            #[cfg(not(feature = "sqlite-persistence"))]
            {
                let now_ms = lifecycle_timestamp_ms(Utc::now());
                let mut queue = state.a2a_compat_task_queue.write().await;
                if let Some(entry) = queue
                    .iter_mut()
                    .find(|entry| entry.session_id == session_id)
                {
                    if let Some(claimed_by) = entry.claimed_by.as_deref() {
                        if claimed_by != sender_id {
                            return Err(ApiError::forbidden(
                                "compat task lease is owned by another claimer",
                            )
                            .with_request_id(rid.clone())
                            .with_details(serde_json::json!({
                                "session_id": session_id.clone(),
                                "task_id": task_id.clone(),
                                "claimed_by": claimed_by,
                                "reporter": sender_id.clone()
                            })));
                        }
                    }
                    entry.claimed_by = Some(sender_id.clone());
                    entry.lease_expires_at_ms = Some(now_ms + A2A_COMPAT_CLAIM_LEASE_MS);
                }
            }

            let progress = evolution_a2a_session_progress(
                State(state.clone()),
                Path(session_id.clone()),
                headers,
                Json(A2aTaskSessionProgressRequest {
                    sender_id: sender_id.clone(),
                    protocol_version: protocol_version.clone(),
                    progress_pct: req.progress_pct.unwrap_or(0),
                    summary: req.summary.clone(),
                    retryable: req.retryable.unwrap_or(false),
                    retry_after_ms: req.retry_after_ms,
                }),
            )
            .await?;
            let envelope = progress.0;
            let ack = envelope.data;
            Ok(Json(ApiEnvelope {
                meta: ApiMeta::ok(),
                request_id: envelope.request_id,
                data: A2aCompatReportResponse {
                    session_id,
                    task_id: ack.task_id,
                    state: ack.state,
                    terminal_state: None,
                    summary: ack.summary,
                },
            }))
        }
        A2aCompatReportStatus::Succeeded
        | A2aCompatReportStatus::Failed
        | A2aCompatReportStatus::Cancelled => {
            let terminal_state = match req.status {
                A2aCompatReportStatus::Succeeded => A2aTaskLifecycleState::Succeeded,
                A2aCompatReportStatus::Failed => A2aTaskLifecycleState::Failed,
                A2aCompatReportStatus::Cancelled => A2aTaskLifecycleState::Cancelled,
                A2aCompatReportStatus::Running => unreachable!("running handled above"),
            };

            #[cfg(feature = "sqlite-persistence")]
            if let Some(repo) = state.runtime_repo.as_ref() {
                if let Some(task) = repo
                    .get_a2a_compat_task(&session_id)
                    .map_err(|e| ApiError::internal(e.to_string()).with_request_id(rid.clone()))?
                {
                    if let (Some(claimed_by), Some(lease_expires_at)) =
                        (task.claimed_by_sender_id.as_deref(), task.lease_expires_at)
                    {
                        if claimed_by != sender_id && lease_expires_at > Utc::now() {
                            return Err(ApiError::forbidden(
                                "compat task lease is owned by another claimer",
                            )
                            .with_request_id(rid.clone())
                            .with_details(serde_json::json!({
                                "session_id": session_id.clone(),
                                "task_id": task_id.clone(),
                                "claimed_by": claimed_by,
                                "reporter": sender_id.clone()
                            })));
                        }
                    }
                }
            } else {
                let queue = state.a2a_compat_task_queue.read().await;
                if let Some(entry) = queue.iter().find(|entry| entry.session_id == session_id) {
                    if let Some(claimed_by) = entry.claimed_by.as_deref() {
                        if claimed_by != sender_id {
                            return Err(ApiError::forbidden(
                                "compat task lease is owned by another claimer",
                            )
                            .with_request_id(rid.clone())
                            .with_details(serde_json::json!({
                                "session_id": session_id.clone(),
                                "task_id": task_id.clone(),
                                "claimed_by": claimed_by,
                                "reporter": sender_id.clone()
                            })));
                        }
                    }
                }
            }
            #[cfg(not(feature = "sqlite-persistence"))]
            {
                let queue = state.a2a_compat_task_queue.read().await;
                if let Some(entry) = queue.iter().find(|entry| entry.session_id == session_id) {
                    if let Some(claimed_by) = entry.claimed_by.as_deref() {
                        if claimed_by != sender_id {
                            return Err(ApiError::forbidden(
                                "compat task lease is owned by another claimer",
                            )
                            .with_request_id(rid.clone())
                            .with_details(serde_json::json!({
                                "session_id": session_id.clone(),
                                "task_id": task_id.clone(),
                                "claimed_by": claimed_by,
                                "reporter": sender_id.clone()
                            })));
                        }
                    }
                }
            }

            let completion = evolution_a2a_session_complete(
                State(state.clone()),
                Path(session_id.clone()),
                headers,
                Json(A2aTaskSessionCompletionRequest {
                    sender_id: sender_id.clone(),
                    protocol_version: protocol_version.clone(),
                    terminal_state: terminal_state.clone(),
                    summary: req.summary,
                    retryable: req.retryable.unwrap_or(false),
                    retry_after_ms: req.retry_after_ms,
                    failure_code: req.failure_code,
                    failure_details: req.failure_details,
                    used_capsule: req.used_capsule.unwrap_or(false),
                    capsule_id: req.capsule_id,
                    reasoning_steps_avoided: req.reasoning_steps_avoided.unwrap_or(0),
                    fallback_reason: req.fallback_reason,
                    task_class_id: req.task_class_id.unwrap_or_else(|| "unknown".into()),
                    task_label: req.task_label.unwrap_or_else(|| task_id.clone()),
                }),
            )
            .await?;
            let envelope = completion.0;
            let result = envelope.data.result;
            let ack = envelope.data.ack;

            #[cfg(feature = "sqlite-persistence")]
            if let Some(repo) = state.runtime_repo.as_ref() {
                let _ = repo
                    .remove_a2a_compat_task(&session_id)
                    .map_err(|e| ApiError::internal(e.to_string()).with_request_id(rid.clone()))?;
            } else {
                let mut queue = state.a2a_compat_task_queue.write().await;
                if let Some(index) = queue
                    .iter()
                    .position(|entry| entry.session_id == session_id)
                {
                    queue.remove(index);
                }
            }
            #[cfg(not(feature = "sqlite-persistence"))]
            {
                let mut queue = state.a2a_compat_task_queue.write().await;
                if let Some(index) = queue
                    .iter()
                    .position(|entry| entry.session_id == session_id)
                {
                    queue.remove(index);
                }
            }
            state
                .runtime_metrics
                .record_a2a_report_to_capture_latency_ms(
                    report_started_at.elapsed().as_secs_f64() * 1000.0,
                );

            Ok(Json(ApiEnvelope {
                meta: ApiMeta::ok(),
                request_id: envelope.request_id,
                data: A2aCompatReportResponse {
                    session_id,
                    task_id: ack.task_id,
                    state: ack.state,
                    terminal_state: Some(result.terminal_state),
                    summary: result.summary,
                },
            }))
        }
    }
}

#[cfg(all(
    feature = "agent-contract-experimental",
    feature = "evolution-network-experimental"
))]
pub async fn evolution_a2a_task_lifecycle(
    State(state): State<ExecutionApiState>,
    Path(task_id): Path<String>,
    Query(q): Query<A2aTaskSessionLookupQuery>,
    headers: HeaderMap,
) -> Result<Json<ApiEnvelope<A2aTaskLifecycleResponse>>, ApiError> {
    let rid = request_id(&headers);
    validate_thread_id(&task_id).map_err(|e| e.with_request_id(rid.clone()))?;
    validate_sender_id(&q.sender_id).map_err(|e| e.with_request_id(rid.clone()))?;
    ensure_task_session_protocol_version(&q.protocol_version, &rid)?;

    let principal = resolve_a2a_principal(&headers, &state);
    ensure_a2a_authorized_action(
        &state,
        &q.sender_id,
        A2aCapability::EvolutionFetch,
        A2aPrivilegeAction::TaskLifecycleRead,
        principal.as_ref(),
        &rid,
    )
    .await?;

    let events = state
        .a2a_task_lifecycle_events
        .read()
        .await
        .get(&task_id)
        .cloned()
        .ok_or_else(|| {
            ApiError::not_found(format!("No A2A lifecycle found for task: {task_id}"))
                .with_request_id(rid.clone())
        })?;

    Ok(Json(ApiEnvelope {
        meta: ApiMeta::ok(),
        request_id: rid,
        data: A2aTaskLifecycleResponse { task_id, events },
    }))
}

#[cfg(all(
    feature = "agent-contract-experimental",
    feature = "evolution-network-experimental"
))]
pub async fn evolution_a2a_session_start(
    State(state): State<ExecutionApiState>,
    headers: HeaderMap,
    Json(req): Json<A2aTaskSessionStartRequest>,
) -> Result<Json<ApiEnvelope<A2aTaskSessionAck>>, ApiError> {
    let rid = request_id(&headers);
    validate_sender_id(&req.sender_id).map_err(|e| e.with_request_id(rid.clone()))?;
    validate_thread_id(&req.task_id).map_err(|e| e.with_request_id(rid.clone()))?;
    ensure_task_session_protocol_version(&req.protocol_version, &rid)?;

    let principal = resolve_a2a_principal(&headers, &state);
    ensure_a2a_authorized_action(
        &state,
        &req.sender_id,
        A2aCapability::Coordination,
        A2aPrivilegeAction::TaskSessionStart,
        principal.as_ref(),
        &rid,
    )
    .await?;

    let now = lifecycle_timestamp_ms(Utc::now());
    let session_id = format!("a2a-session-{}", uuid::Uuid::new_v4());
    let session = A2aTaskSessionSnapshot {
        session_id: session_id.clone(),
        sender_id: req.sender_id.clone(),
        task_id: req.task_id.clone(),
        protocol_version: req.protocol_version,
        state: A2aTaskSessionState::Started,
        created_at_ms: now,
        updated_at_ms: now,
        dispatch_ids: Vec::new(),
        progress: Vec::new(),
        result: None,
    };
    state
        .a2a_task_sessions
        .write()
        .await
        .insert(session_id, session.clone());

    record_task_queued(
        &state,
        &req.task_id,
        "remote a2a task session started and queued",
    )
    .await;

    Ok(Json(ApiEnvelope {
        meta: ApiMeta::ok(),
        request_id: rid,
        data: task_session_ack(&session, &req.task_summary, false, None),
    }))
}

#[cfg(all(
    feature = "agent-contract-experimental",
    feature = "evolution-network-experimental"
))]
pub async fn evolution_a2a_session_dispatch(
    State(state): State<ExecutionApiState>,
    Path(session_id): Path<String>,
    headers: HeaderMap,
    Json(req): Json<A2aTaskSessionDispatchRequest>,
) -> Result<Json<ApiEnvelope<A2aTaskSessionAck>>, ApiError> {
    let rid = request_id(&headers);
    validate_sender_id(&req.sender_id).map_err(|e| e.with_request_id(rid.clone()))?;
    ensure_task_session_protocol_version(&req.protocol_version, &rid)?;
    if req.dispatch_id.trim().is_empty() {
        return Err(ApiError::bad_request("dispatch_id must not be empty").with_request_id(rid));
    }

    let principal = resolve_a2a_principal(&headers, &state);
    ensure_a2a_authorized_action(
        &state,
        &req.sender_id,
        A2aCapability::SupervisedDevloop,
        A2aPrivilegeAction::TaskSessionDispatch,
        principal.as_ref(),
        &rid,
    )
    .await?;

    let now = lifecycle_timestamp_ms(Utc::now());
    let (task_id, ack) = {
        let mut sessions = state.a2a_task_sessions.write().await;
        let session = sessions.get_mut(&session_id).ok_or_else(|| {
            ApiError::not_found(format!("a2a task session not found: {session_id}"))
                .with_request_id(rid.clone())
        })?;
        if session.sender_id != req.sender_id {
            return Err(ApiError::forbidden("a2a task session sender mismatch")
                .with_request_id(rid.clone())
                .with_details(serde_json::json!({
                    "session_id": session_id,
                    "expected_sender_id": session.sender_id,
                    "actual_sender_id": req.sender_id
                })));
        }
        session.state = A2aTaskSessionState::Dispatched;
        session.updated_at_ms = now;
        if !session.dispatch_ids.contains(&req.dispatch_id) {
            session.dispatch_ids.push(req.dispatch_id.clone());
        }
        (
            session.task_id.clone(),
            task_session_ack(session, &req.summary, false, None),
        )
    };

    let dispatch_summary = format!("remote dispatch accepted: {}", req.dispatch_id);
    record_task_running(&state, &task_id, dispatch_summary.as_str()).await;

    Ok(Json(ApiEnvelope {
        meta: ApiMeta::ok(),
        request_id: rid,
        data: ack,
    }))
}

#[cfg(all(
    feature = "agent-contract-experimental",
    feature = "evolution-network-experimental"
))]
pub async fn evolution_a2a_session_progress(
    State(state): State<ExecutionApiState>,
    Path(session_id): Path<String>,
    headers: HeaderMap,
    Json(req): Json<A2aTaskSessionProgressRequest>,
) -> Result<Json<ApiEnvelope<A2aTaskSessionAck>>, ApiError> {
    let rid = request_id(&headers);
    validate_sender_id(&req.sender_id).map_err(|e| e.with_request_id(rid.clone()))?;
    ensure_task_session_protocol_version(&req.protocol_version, &rid)?;
    if req.progress_pct > 100 {
        return Err(ApiError::bad_request("progress_pct must be <= 100").with_request_id(rid));
    }

    let principal = resolve_a2a_principal(&headers, &state);
    ensure_a2a_authorized_action(
        &state,
        &req.sender_id,
        A2aCapability::SupervisedDevloop,
        A2aPrivilegeAction::TaskSessionProgress,
        principal.as_ref(),
        &rid,
    )
    .await?;

    let now = lifecycle_timestamp_ms(Utc::now());
    let (task_id, ack) = {
        let mut sessions = state.a2a_task_sessions.write().await;
        let session = sessions.get_mut(&session_id).ok_or_else(|| {
            ApiError::not_found(format!("a2a task session not found: {session_id}"))
                .with_request_id(rid.clone())
        })?;
        if session.sender_id != req.sender_id {
            return Err(ApiError::forbidden("a2a task session sender mismatch")
                .with_request_id(rid.clone())
                .with_details(serde_json::json!({
                    "session_id": session_id,
                    "expected_sender_id": session.sender_id,
                    "actual_sender_id": req.sender_id
                })));
        }
        session.state = A2aTaskSessionState::InProgress;
        session.updated_at_ms = now;
        session.progress.push(A2aTaskSessionProgressItem {
            progress_pct: req.progress_pct,
            summary: req.summary.clone(),
            retryable: req.retryable,
            retry_after_ms: req.retry_after_ms,
            updated_at_ms: now,
        });
        (
            session.task_id.clone(),
            task_session_ack(session, &req.summary, req.retryable, req.retry_after_ms),
        )
    };

    let progress_summary = format!(
        "remote progress update: {}% {}",
        req.progress_pct, req.summary
    );
    record_task_running(&state, &task_id, progress_summary.as_str()).await;

    Ok(Json(ApiEnvelope {
        meta: ApiMeta::ok(),
        request_id: rid,
        data: ack,
    }))
}

#[cfg(all(
    feature = "agent-contract-experimental",
    feature = "evolution-network-experimental"
))]
pub async fn evolution_a2a_session_complete(
    State(state): State<ExecutionApiState>,
    Path(session_id): Path<String>,
    headers: HeaderMap,
    Json(req): Json<A2aTaskSessionCompletionRequest>,
) -> Result<Json<ApiEnvelope<A2aTaskSessionCompletionResponse>>, ApiError> {
    let rid = request_id(&headers);
    validate_sender_id(&req.sender_id).map_err(|e| e.with_request_id(rid.clone()))?;
    ensure_task_session_protocol_version(&req.protocol_version, &rid)?;
    let session_state = terminal_session_state(&req.terminal_state).ok_or_else(|| {
        ApiError::bad_request("terminal_state must be one of: Succeeded|Failed|Cancelled")
            .with_request_id(rid.clone())
    })?;

    let principal = resolve_a2a_principal(&headers, &state);
    ensure_a2a_authorized_action(
        &state,
        &req.sender_id,
        A2aCapability::SupervisedDevloop,
        A2aPrivilegeAction::TaskSessionComplete,
        principal.as_ref(),
        &rid,
    )
    .await?;
    ensure_a2a_authorized_action(
        &state,
        &req.sender_id,
        A2aCapability::ReplayFeedback,
        A2aPrivilegeAction::TaskSessionComplete,
        principal.as_ref(),
        &rid,
    )
    .await?;

    let replay_feedback = derive_replay_feedback(&req);
    let now = lifecycle_timestamp_ms(Utc::now());
    let (task_id, completion_response) = {
        let mut sessions = state.a2a_task_sessions.write().await;
        let session = sessions.get_mut(&session_id).ok_or_else(|| {
            ApiError::not_found(format!("a2a task session not found: {session_id}"))
                .with_request_id(rid.clone())
        })?;
        if session.sender_id != req.sender_id {
            return Err(ApiError::forbidden("a2a task session sender mismatch")
                .with_request_id(rid.clone())
                .with_details(serde_json::json!({
                    "session_id": session_id,
                    "expected_sender_id": session.sender_id,
                    "actual_sender_id": req.sender_id
                })));
        }

        session.state = session_state;
        session.updated_at_ms = now;
        let result = A2aTaskSessionResult {
            terminal_state: req.terminal_state.clone(),
            summary: req.summary.clone(),
            retryable: req.retryable,
            retry_after_ms: req.retry_after_ms,
            failure_code: req.failure_code.clone(),
            failure_details: req.failure_details.clone(),
            replay_feedback,
        };
        session.result = Some(result.clone());
        let ack = task_session_ack(session, &req.summary, req.retryable, req.retry_after_ms);
        (
            session.task_id.clone(),
            A2aTaskSessionCompletionResponse { ack, result },
        )
    };

    match req.terminal_state {
        A2aTaskLifecycleState::Succeeded => {
            record_task_succeeded(&state, &task_id, "remote a2a task session succeeded").await;
        }
        A2aTaskLifecycleState::Failed => {
            let details = req
                .failure_details
                .clone()
                .or_else(|| Some(req.summary.clone()));
            record_task_failed(&state, &task_id, "remote a2a task session failed", details).await;
        }
        A2aTaskLifecycleState::Cancelled => {
            record_task_cancelled(&state, &task_id, "remote a2a task session cancelled").await;
        }
        A2aTaskLifecycleState::Queued | A2aTaskLifecycleState::Running => {
            // Handled by terminal state validation.
        }
    }

    Ok(Json(ApiEnvelope {
        meta: ApiMeta::ok(),
        request_id: rid,
        data: completion_response,
    }))
}

#[cfg(all(
    feature = "agent-contract-experimental",
    feature = "evolution-network-experimental"
))]
pub async fn evolution_a2a_export_session(
    State(state): State<ExecutionApiState>,
    Path(sender_id): Path<String>,
    Query(q): Query<A2aSessionReplicationExportQuery>,
    headers: HeaderMap,
) -> Result<Json<ApiEnvelope<A2aSessionReplicationPayload>>, ApiError> {
    let rid = request_id(&headers);
    validate_sender_id(&sender_id).map_err(|e| e.with_request_id(rid.clone()))?;
    ensure_task_session_protocol_version(&q.protocol_version, &rid)?;
    let now = Utc::now();

    #[cfg(feature = "sqlite-persistence")]
    let mut session = state.a2a_sessions.read().await.get(&sender_id).cloned();
    #[cfg(not(feature = "sqlite-persistence"))]
    let session = state.a2a_sessions.read().await.get(&sender_id).cloned();

    #[cfg(feature = "sqlite-persistence")]
    if session.is_none() {
        if let Some(repo) = state.runtime_repo.as_ref() {
            if let Some(stored) = repo
                .get_active_a2a_session(&sender_id, now)
                .map_err(|e| ApiError::internal(e.to_string()).with_request_id(rid.to_string()))?
            {
                let enabled_capabilities: Vec<A2aCapability> =
                    serde_json::from_str(&stored.enabled_capabilities_json).map_err(|e| {
                        ApiError::internal(format!("invalid persisted a2a capabilities: {}", e))
                            .with_request_id(rid.to_string())
                    })?;
                let principal_from_store = match (stored.actor_type, stored.actor_role) {
                    (Some(actor_type), Some(actor_role)) => Some(A2aSessionPrincipal {
                        actor_type,
                        actor_id: stored.actor_id,
                        actor_role,
                    }),
                    (None, None) => None,
                    _ => {
                        return Err(ApiError::internal(
                            "invalid persisted a2a principal binding shape",
                        )
                        .with_request_id(rid.to_string()))
                    }
                };
                let hydrated = A2aSession {
                    negotiated_protocol: A2aProtocol {
                        name: stored.protocol,
                        version: stored.protocol_version,
                    },
                    privilege_profile: privilege_profile_from_capabilities(&enabled_capabilities),
                    enabled_capabilities,
                    principal: principal_from_store,
                    expires_at: stored.expires_at,
                };
                state
                    .a2a_sessions
                    .write()
                    .await
                    .insert(sender_id.clone(), hydrated.clone());
                session = Some(hydrated);
            }
        }
    }

    let Some(session) = session else {
        return Err(
            ApiError::not_found(format!("a2a session not found for sender: {sender_id}"))
                .with_request_id(rid),
        );
    };
    if session.expires_at <= now {
        state.a2a_sessions.write().await.remove(&sender_id);
        return Err(
            ApiError::not_found(format!("a2a session expired for sender: {sender_id}"))
                .with_request_id(request_id(&headers)),
        );
    }

    let payload = A2aSessionReplicationPayload {
        sender_id,
        protocol: session.negotiated_protocol,
        enabled_capabilities: session.enabled_capabilities,
        actor_type: session.principal.as_ref().map(|p| p.actor_type.clone()),
        actor_id: session.principal.as_ref().and_then(|p| p.actor_id.clone()),
        actor_role: session.principal.as_ref().map(|p| p.actor_role.clone()),
        expires_at_ms: lifecycle_timestamp_ms(session.expires_at),
    };

    Ok(Json(ApiEnvelope {
        meta: ApiMeta::ok(),
        request_id: request_id(&headers),
        data: payload,
    }))
}

#[cfg(all(
    feature = "agent-contract-experimental",
    feature = "evolution-network-experimental"
))]
pub async fn evolution_a2a_import_session(
    State(state): State<ExecutionApiState>,
    headers: HeaderMap,
    Json(req): Json<A2aSessionReplicationImportRequest>,
) -> Result<Json<ApiEnvelope<A2aSessionReplicationResponse>>, ApiError> {
    let rid = request_id(&headers);
    ensure_task_session_protocol_version(&req.protocol_version, &rid)?;
    validate_sender_id(&req.session.sender_id).map_err(|e| e.with_request_id(rid.clone()))?;
    if req.source_node_id.trim().is_empty() {
        return Err(ApiError::bad_request("source_node_id must not be empty").with_request_id(rid));
    }
    if req.session.protocol.name != crate::agent_contract::A2A_PROTOCOL_NAME
        || !crate::agent_contract::A2A_SUPPORTED_PROTOCOL_VERSIONS
            .iter()
            .any(|supported| req.session.protocol.version == *supported)
    {
        return Err(
            ApiError::bad_request("session protocol payload is incompatible")
                .with_request_id(rid)
                .with_details(serde_json::json!({
                    "expected_name": crate::agent_contract::A2A_PROTOCOL_NAME,
                    "expected_versions": crate::agent_contract::A2A_SUPPORTED_PROTOCOL_VERSIONS,
                    "actual_name": req.session.protocol.name,
                    "actual_version": req.session.protocol.version,
                })),
        );
    }

    let expires_at = datetime_from_unix_ms(req.session.expires_at_ms, &rid, "expires_at_ms")?;
    if expires_at <= Utc::now() {
        return Err(ApiError::bad_request("cannot import expired a2a session")
            .with_request_id(rid)
            .with_details(serde_json::json!({
                "sender_id": req.session.sender_id,
                "expires_at_ms": req.session.expires_at_ms
            })));
    }

    let principal = match (
        req.session.actor_type.clone(),
        req.session.actor_id.clone(),
        req.session.actor_role.clone(),
    ) {
        (Some(actor_type), actor_id, Some(actor_role)) => Some(A2aSessionPrincipal {
            actor_type,
            actor_id,
            actor_role,
        }),
        (None, None, None) => None,
        _ => {
            return Err(
                ApiError::bad_request("invalid session principal payload shape")
                    .with_request_id(rid),
            )
        }
    };

    let session = A2aSession {
        negotiated_protocol: req.session.protocol.clone(),
        privilege_profile: privilege_profile_from_capabilities(&req.session.enabled_capabilities),
        enabled_capabilities: req.session.enabled_capabilities.clone(),
        principal,
        expires_at,
    };
    state
        .a2a_sessions
        .write()
        .await
        .insert(req.session.sender_id.clone(), session.clone());

    #[cfg(feature = "sqlite-persistence")]
    if let Some(repo) = state.runtime_repo.as_ref() {
        let now = Utc::now();
        repo.upsert_a2a_session(&A2aSessionRow {
            sender_id: req.session.sender_id.clone(),
            protocol: session.negotiated_protocol.name.clone(),
            protocol_version: session.negotiated_protocol.version.clone(),
            enabled_capabilities_json: serde_json::to_string(&session.enabled_capabilities)
                .map_err(|e| {
                    ApiError::internal(format!("serialize negotiated a2a capabilities: {}", e))
                        .with_request_id(request_id(&headers))
                })?,
            actor_type: session.principal.as_ref().map(|p| p.actor_type.clone()),
            actor_id: session.principal.as_ref().and_then(|p| p.actor_id.clone()),
            actor_role: session.principal.as_ref().map(|p| p.actor_role.clone()),
            negotiated_at: now,
            expires_at: session.expires_at,
            updated_at: now,
        })
        .map_err(|e| ApiError::internal(e.to_string()).with_request_id(request_id(&headers)))?;
    }

    Ok(Json(ApiEnvelope {
        meta: ApiMeta::ok(),
        request_id: request_id(&headers),
        data: A2aSessionReplicationResponse {
            imported: true,
            source_node_id: req.source_node_id,
            sender_id: req.session.sender_id,
            expires_at_ms: req.session.expires_at_ms,
        },
    }))
}

#[cfg(all(
    feature = "agent-contract-experimental",
    feature = "evolution-network-experimental"
))]
pub async fn evolution_a2a_session_snapshot(
    State(state): State<ExecutionApiState>,
    Path(session_id): Path<String>,
    Query(q): Query<A2aTaskSessionLookupQuery>,
    headers: HeaderMap,
) -> Result<Json<ApiEnvelope<A2aTaskSessionSnapshot>>, ApiError> {
    let rid = request_id(&headers);
    validate_sender_id(&q.sender_id).map_err(|e| e.with_request_id(rid.clone()))?;
    ensure_task_session_protocol_version(&q.protocol_version, &rid)?;

    let principal = resolve_a2a_principal(&headers, &state);
    ensure_a2a_authorized_action(
        &state,
        &q.sender_id,
        A2aCapability::Coordination,
        A2aPrivilegeAction::TaskSessionSnapshot,
        principal.as_ref(),
        &rid,
    )
    .await?;

    let snapshot = state
        .a2a_task_sessions
        .read()
        .await
        .get(&session_id)
        .cloned()
        .ok_or_else(|| {
            ApiError::not_found(format!("a2a task session not found: {session_id}"))
                .with_request_id(rid.clone())
        })?;
    if snapshot.sender_id != q.sender_id {
        return Err(ApiError::forbidden("a2a task session sender mismatch")
            .with_request_id(rid)
            .with_details(serde_json::json!({
                "session_id": session_id,
                "expected_sender_id": snapshot.sender_id,
                "actual_sender_id": q.sender_id
            })));
    }

    Ok(Json(ApiEnvelope {
        meta: ApiMeta::ok(),
        request_id: rid,
        data: snapshot,
    }))
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

    record_task_queued(
        &state,
        &req.thread_id,
        "task accepted into runtime execution queue",
    )
    .await;
    record_task_running(&state, &req.thread_id, "task execution started").await;

    let run_trace = TraceContextState::new_from_headers(&headers, &rid)?;
    let run_span = lifecycle_span(
        "job.run",
        &rid,
        Some(&req.thread_id),
        None,
        None,
        Some(&run_trace),
    );

    let result = match state
        .graph_bridge
        .run(&req.thread_id, &input)
        .instrument(run_span)
        .await
    {
        Ok(result) => result,
        Err(e) => {
            let error_message = e.to_string();
            record_task_failed(
                &state,
                &req.thread_id,
                "task execution failed",
                Some(error_message.clone()),
            )
            .await;
            return Err(ApiError::internal(format!("run failed: {}", error_message))
                .with_request_id(rid.clone()));
        }
    };

    let interrupts = result.interrupts;
    let status = if interrupts.is_empty() {
        "completed".to_string()
    } else {
        "interrupted".to_string()
    };
    if interrupts.is_empty() {
        record_task_succeeded(&state, &req.thread_id, "task completed successfully").await;
    } else {
        record_task_running(
            &state,
            &req.thread_id,
            "task interrupted and waiting for resume",
        )
        .await;
    }

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
    log::info!(
        "execution_inspect request_id={} thread_id={} checkpoint_id=none",
        rid,
        thread_id
    );
    let snapshot = state
        .graph_bridge
        .snapshot(&thread_id, None)
        .await
        .map_err(|e| {
            if e.kind == ExecutionGraphBridgeErrorKind::NotFound {
                ApiError::not_found(e.message).with_request_id(rid.clone())
            } else {
                ApiError::internal(e.message).with_request_id(rid.clone())
            }
        })?;

    let checkpoint_id = snapshot.checkpoint_id.clone();
    let created_at = snapshot.created_at.to_rfc3339();
    let values = snapshot.values;

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
    log::info!(
        "execution_history request_id={} thread_id={} checkpoint_id=none",
        rid,
        thread_id
    );
    let history = state.graph_bridge.history(&thread_id).await.map_err(|e| {
        ApiError::internal(format!("history failed: {}", e.message)).with_request_id(rid.clone())
    })?;

    let items = history
        .iter()
        .map(|s| JobHistoryItem {
            checkpoint_id: s.checkpoint_id.clone(),
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
    log::info!(
        "execution_timeline request_id={} thread_id={} checkpoint_id=none",
        rid,
        thread_id
    );
    let history = state.graph_bridge.history(&thread_id).await.map_err(|e| {
        ApiError::internal(format!("timeline failed: {}", e.message)).with_request_id(rid.clone())
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
            checkpoint_id: s.checkpoint_id.clone(),
            created_at: s.created_at.to_rfc3339(),
        })
        .collect();
    let (observability, trace) = observability_and_trace_from_history(&state, &thread_id, &history);

    Ok(Json(ApiEnvelope {
        meta: ApiMeta::ok(),
        request_id: rid,
        data: JobTimelineResponse {
            thread_id,
            timeline,
            observability,
            trace: trace_response(trace),
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
    let snapshot = state
        .graph_bridge
        .snapshot(&thread_id, Some(&checkpoint_id))
        .await
        .map_err(|e| {
            if e.kind == ExecutionGraphBridgeErrorKind::NotFound {
                ApiError::not_found(e.message).with_request_id(rid.clone())
            } else {
                ApiError::internal(e.message).with_request_id(rid.clone())
            }
        })?;
    let created_at = snapshot.created_at.to_rfc3339();
    let values = snapshot.values;

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

    log::info!(
        "execution_resume request_id={} thread_id={} checkpoint_id={}",
        rid,
        thread_id,
        req.checkpoint_id
            .clone()
            .unwrap_or_else(|| "none".to_string())
    );

    record_task_running(&state, &thread_id, "task resume execution started").await;

    let result = match state
        .graph_bridge
        .resume(&thread_id, req.checkpoint_id.as_deref(), req.value)
        .await
    {
        Ok(result) => result,
        Err(e) => {
            let error_message = e.to_string();
            record_task_failed(
                &state,
                &thread_id,
                "task resume execution failed",
                Some(error_message.clone()),
            )
            .await;
            return Err(
                ApiError::internal(format!("resume failed: {}", error_message))
                    .with_request_id(rid.clone()),
            );
        }
    };

    let interrupts: Vec<Value> = result.interrupts;
    let status = if interrupts.is_empty() {
        "completed".to_string()
    } else {
        "interrupted".to_string()
    };
    if interrupts.is_empty() {
        record_task_succeeded(&state, &thread_id, "task resume completed successfully").await;
    } else {
        record_task_running(
            &state,
            &thread_id,
            "task interrupted again and waiting for resume",
        )
        .await;
    }

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

    log::info!(
        "execution_replay request_id={} thread_id={} checkpoint_id={}",
        rid,
        thread_id,
        req.checkpoint_id
            .clone()
            .unwrap_or_else(|| "none".to_string())
    );

    record_task_running(&state, &thread_id, "task replay execution started").await;

    match state
        .graph_bridge
        .replay(&thread_id, req.checkpoint_id.as_deref())
        .await
    {
        Ok(()) => {}
        Err(e) => {
            let error_message = e.to_string();
            #[cfg(feature = "sqlite-persistence")]
            if let (Some(repo), Some(fingerprint)) =
                (state.runtime_repo.as_ref(), replay_guard.as_deref())
            {
                let _ = repo.abandon_replay_effect(fingerprint);
            }
            record_task_failed(
                &state,
                &thread_id,
                "task replay execution failed",
                Some(error_message.clone()),
            )
            .await;
            return Err(
                ApiError::internal(format!("replay failed: {}", error_message))
                    .with_request_id(rid.clone()),
            );
        }
    }

    record_task_succeeded(&state, &thread_id, "task replay completed successfully").await;

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
    let reason = req.reason.clone();
    let cancel_summary = reason
        .clone()
        .unwrap_or_else(|| "task cancelled via API request".to_string());
    record_task_cancelled(&state, &thread_id, cancel_summary.as_str()).await;
    Ok(Json(ApiEnvelope {
        meta: ApiMeta::ok(),
        request_id: rid,
        data: CancelJobResponse {
            thread_id,
            status: "cancelled".to_string(),
            reason,
        },
    }))
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
    let snapshot = state
        .graph_bridge
        .snapshot(&thread_id, None)
        .await
        .map_err(|e| {
            if e.kind == ExecutionGraphBridgeErrorKind::NotFound {
                ApiError::not_found(e.message).with_request_id(rid.clone())
            } else {
                ApiError::internal(e.message).with_request_id(rid.clone())
            }
        })?;
    let history = state
        .graph_bridge
        .history(&thread_id)
        .await
        .unwrap_or_default();
    let history_items = history
        .iter()
        .map(|s| JobHistoryItem {
            checkpoint_id: s.checkpoint_id.clone(),
            created_at: s.created_at.to_rfc3339(),
        })
        .collect();
    let timeline = history
        .iter()
        .enumerate()
        .map(|(i, s)| JobTimelineItem {
            seq: (i + 1) as u64,
            event_type: "checkpoint_saved".to_string(),
            checkpoint_id: s.checkpoint_id.clone(),
            created_at: s.created_at.to_rfc3339(),
        })
        .collect();
    let (observability, trace) = observability_and_trace_from_history(&state, &thread_id, &history);
    let values = snapshot.values;
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
            checkpoint_id: snapshot.checkpoint_id.clone(),
            values,
            history: history_items,
            timeline,
            pending_interrupt,
            trace: trace_response(trace),
            observability,
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
    let history = state.graph_bridge.history(&thread_id).await.map_err(|e| {
        ApiError::internal(format!("timeline export failed: {}", e.message))
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
            checkpoint_id: s.checkpoint_id.clone(),
            created_at: s.created_at.to_rfc3339(),
        })
        .collect();
    let history_items = history
        .iter()
        .map(|s| JobHistoryItem {
            checkpoint_id: s.checkpoint_id.clone(),
            created_at: s.created_at.to_rfc3339(),
        })
        .collect();
    let (observability, trace) = observability_and_trace_from_history(&state, &thread_id, &history);
    Ok(Json(ApiEnvelope {
        meta: ApiMeta::ok(),
        request_id: rid,
        data: TimelineExportResponse {
            thread_id,
            timeline,
            history: history_items,
            observability,
            trace: trace_response(trace),
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

    let report_summary = format!(
        "worker step report accepted: action={} status={}",
        req.action_id, req.status
    );
    record_task_running(&state, &req.attempt_id, report_summary.as_str()).await;
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
        let response_status = match outcome.status {
            AttemptExecutionStatus::RetryBackoff => "retry_scheduled".to_string(),
            AttemptExecutionStatus::Completed => "completed".to_string(),
            AttemptExecutionStatus::Failed => "failed".to_string(),
            AttemptExecutionStatus::Cancelled => "cancelled".to_string(),
            AttemptExecutionStatus::Queued => "queued".to_string(),
            AttemptExecutionStatus::Leased => "leased".to_string(),
            AttemptExecutionStatus::Running => "running".to_string(),
        };
        let lifecycle_summary = format!("worker ack outcome: {}", response_status);
        match outcome.status {
            AttemptExecutionStatus::Completed => {
                record_task_succeeded(&state, &req.attempt_id, lifecycle_summary.as_str()).await;
            }
            AttemptExecutionStatus::Failed => {
                record_task_failed(
                    &state,
                    &req.attempt_id,
                    "worker reported terminal failure",
                    Some(lifecycle_summary.clone()),
                )
                .await;
            }
            AttemptExecutionStatus::Cancelled => {
                record_task_cancelled(&state, &req.attempt_id, lifecycle_summary.as_str()).await;
            }
            AttemptExecutionStatus::RetryBackoff
            | AttemptExecutionStatus::Queued
            | AttemptExecutionStatus::Leased
            | AttemptExecutionStatus::Running => {
                record_task_running(&state, &req.attempt_id, lifecycle_summary.as_str()).await;
            }
        }
        let _span = lifecycle_span(
            "attempt.ack",
            &rid,
            None,
            Some(&req.attempt_id),
            Some(&worker_id),
            trace.as_ref(),
        )
        .entered();
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

    use crate::execution_runtime::models::AttemptExecutionStatus;
    use crate::execution_runtime::repository::RuntimeRepository;
    #[cfg(feature = "sqlite-persistence")]
    use crate::execution_runtime::sqlite_runtime_repository::TimeoutPolicyConfig;
    use crate::graph::{
        function_node, interrupt, GraphError, InMemorySaver, MessagesState, StateGraph, END, START,
    };
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

    #[cfg(all(
        feature = "agent-contract-experimental",
        feature = "evolution-network-experimental"
    ))]
    async fn handshake_agent_with_caps(
        router: &axum::Router,
        agent_id: &str,
        capabilities: &[&str],
    ) -> serde_json::Value {
        handshake_agent_with_caps_and_level(router, agent_id, "A4", capabilities).await
    }

    #[cfg(all(
        feature = "agent-contract-experimental",
        feature = "evolution-network-experimental"
    ))]
    async fn handshake_agent_with_caps_and_level(
        router: &axum::Router,
        agent_id: &str,
        capability_level: &str,
        capabilities: &[&str],
    ) -> serde_json::Value {
        let handshake_req = Request::builder()
            .method(Method::POST)
            .uri("/v1/evolution/a2a/handshake")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "agent_id": agent_id,
                    "role": "Planner",
                    "capability_level": capability_level,
                    "supported_protocols": [
                        {
                            "name": crate::agent_contract::A2A_PROTOCOL_NAME,
                            "version": crate::agent_contract::A2A_PROTOCOL_VERSION
                        }
                    ],
                    "advertised_capabilities": capabilities
                })
                .to_string(),
            ))
            .unwrap();
        let handshake_resp = router.clone().oneshot(handshake_req).await.unwrap();
        assert_eq!(handshake_resp.status(), StatusCode::OK);
        let handshake_body = axum::body::to_bytes(handshake_resp.into_body(), usize::MAX)
            .await
            .expect("handshake body");
        serde_json::from_slice(&handshake_body).expect("handshake json")
    }

    #[cfg(all(
        feature = "agent-contract-experimental",
        feature = "evolution-network-experimental"
    ))]
    async fn handshake_agent_with_caps_and_protocols(
        router: &axum::Router,
        endpoint: &str,
        agent_id: &str,
        capability_level: &str,
        capabilities: &[&str],
        supported_protocols: &[&str],
    ) -> serde_json::Value {
        let protocols = supported_protocols
            .iter()
            .map(|version| {
                serde_json::json!({
                    "name": crate::agent_contract::A2A_PROTOCOL_NAME,
                    "version": version
                })
            })
            .collect::<Vec<_>>();
        let handshake_req = Request::builder()
            .method(Method::POST)
            .uri(endpoint)
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "agent_id": agent_id,
                    "role": "Planner",
                    "capability_level": capability_level,
                    "supported_protocols": protocols,
                    "advertised_capabilities": capabilities
                })
                .to_string(),
            ))
            .unwrap();
        let handshake_resp = router.clone().oneshot(handshake_req).await.unwrap();
        assert_eq!(handshake_resp.status(), StatusCode::OK);
        let handshake_body = axum::body::to_bytes(handshake_resp.into_body(), usize::MAX)
            .await
            .expect("handshake body");
        serde_json::from_slice(&handshake_body).expect("handshake json")
    }

    #[cfg(all(
        feature = "agent-contract-experimental",
        feature = "evolution-network-experimental"
    ))]
    async fn fetch_lifecycle_events(
        router: &axum::Router,
        task_id: &str,
        sender_id: &str,
    ) -> serde_json::Value {
        let req = Request::builder()
            .method(Method::GET)
            .uri(format!(
                "/v1/evolution/a2a/tasks/{task_id}/lifecycle?sender_id={sender_id}&protocol_version={}",
                crate::agent_contract::A2A_TASK_SESSION_PROTOCOL_VERSION
            ))
            .body(Body::empty())
            .unwrap();
        let resp = router.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .expect("lifecycle body");
        serde_json::from_slice(&body).expect("lifecycle json")
    }

    #[cfg(feature = "evolution-network-experimental")]
    #[tokio::test]
    async fn evolution_publish_fetch_and_revoke_routes_work() {
        let store_root =
            std::env::temp_dir().join(format!("oris-evolution-api-test-{}", uuid::Uuid::new_v4()));
        let _ = std::fs::remove_dir_all(&store_root);
        let router = build_router(
            ExecutionApiState::new(build_test_graph().await).with_evolution_store(Arc::new(
                crate::evolution::JsonlEvolutionStore::new(&store_root),
            )),
        );

        #[cfg(feature = "agent-contract-experimental")]
        {
            let handshake_json = handshake_agent_with_caps(
                &router,
                "node-a",
                &["EvolutionPublish", "EvolutionFetch", "EvolutionRevoke"],
            )
            .await;
            assert_eq!(handshake_json["data"]["accepted"], true);

            let consumer_handshake_json =
                handshake_agent_with_caps(&router, "consumer-a", &["EvolutionFetch"]).await;
            assert_eq!(consumer_handshake_json["data"]["accepted"], true);
        }

        let publish_req = Request::builder()
            .method(Method::POST)
            .uri("/v1/evolution/publish")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "sender_id": "node-a",
                    "assets": [
                        {
                            "kind": "gene",
                            "gene": {
                                "id": "gene-rust",
                                "signals": ["rust", "http"],
                                "strategy": ["prefer replay"],
                                "validation": ["cargo test -p oris-runtime"]
                            }
                        }
                    ]
                })
                .to_string(),
            ))
            .unwrap();
        let publish_resp = router.clone().oneshot(publish_req).await.unwrap();
        assert_eq!(publish_resp.status(), StatusCode::OK);
        let publish_body = axum::body::to_bytes(publish_resp.into_body(), usize::MAX)
            .await
            .expect("publish body");
        let publish_json: serde_json::Value =
            serde_json::from_slice(&publish_body).expect("publish json");
        assert_eq!(publish_json["data"]["accepted"], true);
        assert_eq!(
            publish_json["data"]["imported_asset_ids"]
                .as_array()
                .map(Vec::len),
            Some(1)
        );

        let fetch_req = Request::builder()
            .method(Method::POST)
            .uri("/v1/evolution/fetch")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "sender_id": "consumer-a",
                    "signals": ["rust"]
                })
                .to_string(),
            ))
            .unwrap();
        let fetch_resp = router.clone().oneshot(fetch_req).await.unwrap();
        assert_eq!(fetch_resp.status(), StatusCode::OK);
        let fetch_body = axum::body::to_bytes(fetch_resp.into_body(), usize::MAX)
            .await
            .expect("fetch body");
        let fetch_json: serde_json::Value =
            serde_json::from_slice(&fetch_body).expect("fetch json");
        assert_eq!(fetch_json["data"]["sender_id"], "execution-api");
        // Remotely published genes stay quarantined until a successful local replay promotes them.
        assert_eq!(
            fetch_json["data"]["assets"].as_array().map(Vec::len),
            Some(0)
        );

        let revoke_req = Request::builder()
            .method(Method::POST)
            .uri("/v1/evolution/revoke")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "sender_id": "node-a",
                    "asset_ids": ["gene-rust"],
                    "reason": "replay regression"
                })
                .to_string(),
            ))
            .unwrap();
        let revoke_resp = router.clone().oneshot(revoke_req).await.unwrap();
        assert_eq!(revoke_resp.status(), StatusCode::OK);

        let fetch_after_revoke_req = Request::builder()
            .method(Method::POST)
            .uri("/v1/evolution/fetch")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "sender_id": "consumer-a",
                    "signals": ["rust"]
                })
                .to_string(),
            ))
            .unwrap();
        let fetch_after_revoke_resp = router.oneshot(fetch_after_revoke_req).await.unwrap();
        assert_eq!(fetch_after_revoke_resp.status(), StatusCode::OK);
        let fetch_after_revoke_body =
            axum::body::to_bytes(fetch_after_revoke_resp.into_body(), usize::MAX)
                .await
                .expect("fetch after revoke body");
        let fetch_after_revoke_json: serde_json::Value =
            serde_json::from_slice(&fetch_after_revoke_body).expect("fetch after revoke json");
        assert_eq!(
            fetch_after_revoke_json["data"]["assets"]
                .as_array()
                .map(Vec::len),
            Some(0)
        );
        let _ = std::fs::remove_dir_all(&store_root);
    }

    #[cfg(all(
        feature = "agent-contract-experimental",
        feature = "evolution-network-experimental"
    ))]
    #[tokio::test]
    async fn evolution_publish_requires_handshake_when_agent_contract_enabled() {
        let router = build_router(ExecutionApiState::new(build_test_graph().await));
        let publish_req = Request::builder()
            .method(Method::POST)
            .uri("/v1/evolution/publish")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "sender_id": "node-a",
                    "assets": []
                })
                .to_string(),
            ))
            .unwrap();
        let publish_resp = router.oneshot(publish_req).await.unwrap();
        assert_eq!(publish_resp.status(), StatusCode::FORBIDDEN);
        let publish_body = axum::body::to_bytes(publish_resp.into_body(), usize::MAX)
            .await
            .expect("publish body");
        let publish_json: serde_json::Value =
            serde_json::from_slice(&publish_body).expect("publish json");
        assert_eq!(
            publish_json["error"]["message"],
            "a2a handshake required before calling evolution routes"
        );
    }

    #[cfg(all(
        feature = "agent-contract-experimental",
        feature = "evolution-network-experimental"
    ))]
    #[tokio::test]
    async fn evolution_publish_rejects_missing_negotiated_capability() {
        let router = build_router(ExecutionApiState::new(build_test_graph().await));
        let handshake_json =
            handshake_agent_with_caps(&router, "node-a", &["EvolutionFetch"]).await;
        assert_eq!(handshake_json["data"]["accepted"], true);

        let publish_req = Request::builder()
            .method(Method::POST)
            .uri("/v1/evolution/publish")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "sender_id": "node-a",
                    "assets": []
                })
                .to_string(),
            ))
            .unwrap();
        let publish_resp = router.oneshot(publish_req).await.unwrap();
        assert_eq!(publish_resp.status(), StatusCode::FORBIDDEN);
        let publish_body = axum::body::to_bytes(publish_resp.into_body(), usize::MAX)
            .await
            .expect("publish body");
        let publish_json: serde_json::Value =
            serde_json::from_slice(&publish_body).expect("publish json");
        assert_eq!(
            publish_json["error"]["message"],
            "negotiated capabilities do not allow this evolution action"
        );
    }

    #[cfg(all(
        feature = "agent-contract-experimental",
        feature = "evolution-network-experimental",
        feature = "sqlite-persistence"
    ))]
    #[tokio::test]
    async fn evolution_a2a_session_survives_restart_with_sqlite_persistence() {
        let db_path =
            std::env::temp_dir().join(format!("oris-a2a-session-{}.sqlite", uuid::Uuid::new_v4()));
        let db_path_str = db_path.to_string_lossy().to_string();
        let store_root =
            std::env::temp_dir().join(format!("oris-evolution-a2a-store-{}", uuid::Uuid::new_v4()));
        let _ = std::fs::remove_file(&db_path);
        let _ = std::fs::remove_dir_all(&store_root);

        let router = build_router(
            ExecutionApiState::with_sqlite_idempotency(build_test_graph().await, &db_path_str)
                .with_evolution_store(Arc::new(crate::evolution::JsonlEvolutionStore::new(
                    &store_root,
                ))),
        );
        let handshake_json = handshake_agent_with_caps(
            &router,
            "node-a",
            &["EvolutionPublish", "EvolutionFetch", "EvolutionRevoke"],
        )
        .await;
        assert_eq!(handshake_json["data"]["accepted"], true);
        drop(router);

        let restarted_router = build_router(
            ExecutionApiState::with_sqlite_idempotency(build_test_graph().await, &db_path_str)
                .with_evolution_store(Arc::new(crate::evolution::JsonlEvolutionStore::new(
                    &store_root,
                ))),
        );
        let publish_req = Request::builder()
            .method(Method::POST)
            .uri("/v1/evolution/publish")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "sender_id": "node-a",
                    "assets": []
                })
                .to_string(),
            ))
            .unwrap();
        let publish_resp = restarted_router.oneshot(publish_req).await.unwrap();
        assert_eq!(publish_resp.status(), StatusCode::OK);

        let _ = std::fs::remove_file(&db_path);
        let _ = std::fs::remove_dir_all(&store_root);
    }

    #[cfg(all(
        feature = "agent-contract-experimental",
        feature = "evolution-network-experimental",
        feature = "sqlite-persistence"
    ))]
    #[tokio::test]
    async fn evolution_a2a_session_replication_propagates_across_nodes() {
        let db_a = std::env::temp_dir().join(format!(
            "oris-a2a-replicate-a-{}.sqlite",
            uuid::Uuid::new_v4()
        ));
        let db_b = std::env::temp_dir().join(format!(
            "oris-a2a-replicate-b-{}.sqlite",
            uuid::Uuid::new_v4()
        ));
        let db_a_str = db_a.to_string_lossy().to_string();
        let db_b_str = db_b.to_string_lossy().to_string();
        let store_a = std::env::temp_dir().join(format!(
            "oris-a2a-replicate-store-a-{}",
            uuid::Uuid::new_v4()
        ));
        let store_b = std::env::temp_dir().join(format!(
            "oris-a2a-replicate-store-b-{}",
            uuid::Uuid::new_v4()
        ));
        let _ = std::fs::remove_file(&db_a);
        let _ = std::fs::remove_file(&db_b);
        let _ = std::fs::remove_dir_all(&store_a);
        let _ = std::fs::remove_dir_all(&store_b);

        let router_a = build_router(
            ExecutionApiState::with_sqlite_idempotency(build_test_graph().await, &db_a_str)
                .with_evolution_store(Arc::new(crate::evolution::JsonlEvolutionStore::new(
                    &store_a,
                ))),
        );
        let handshake_json = handshake_agent_with_caps_and_level(
            &router_a,
            "replicated-sender",
            "A4",
            &["EvolutionPublish", "EvolutionFetch"],
        )
        .await;
        assert_eq!(handshake_json["data"]["accepted"], true);

        let export_req = Request::builder()
            .method(Method::GET)
            .uri(format!(
                "/v1/evolution/a2a/sessions/replicated-sender/replicate?protocol_version={}",
                crate::agent_contract::A2A_TASK_SESSION_PROTOCOL_VERSION
            ))
            .body(Body::empty())
            .unwrap();
        let export_resp = router_a.clone().oneshot(export_req).await.unwrap();
        assert_eq!(export_resp.status(), StatusCode::OK);
        let export_body = axum::body::to_bytes(export_resp.into_body(), usize::MAX)
            .await
            .expect("export body");
        let export_json: serde_json::Value =
            serde_json::from_slice(&export_body).expect("export json");
        let session_payload = export_json["data"].clone();

        let router_b = build_router(
            ExecutionApiState::with_sqlite_idempotency(build_test_graph().await, &db_b_str)
                .with_evolution_store(Arc::new(crate::evolution::JsonlEvolutionStore::new(
                    &store_b,
                ))),
        );
        let import_req = Request::builder()
            .method(Method::POST)
            .uri("/v1/evolution/a2a/sessions/replicate")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "source_node_id": "node-a",
                    "protocol_version": crate::agent_contract::A2A_TASK_SESSION_PROTOCOL_VERSION,
                    "session": session_payload
                })
                .to_string(),
            ))
            .unwrap();
        let import_resp = router_b.clone().oneshot(import_req).await.unwrap();
        assert_eq!(import_resp.status(), StatusCode::OK);
        let import_body = axum::body::to_bytes(import_resp.into_body(), usize::MAX)
            .await
            .expect("import body");
        let import_json: serde_json::Value =
            serde_json::from_slice(&import_body).expect("import json");
        assert_eq!(import_json["data"]["imported"], true);

        let publish_req = Request::builder()
            .method(Method::POST)
            .uri("/v1/evolution/publish")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "sender_id": "replicated-sender",
                    "assets": []
                })
                .to_string(),
            ))
            .unwrap();
        let publish_resp = router_b.oneshot(publish_req).await.unwrap();
        assert_eq!(publish_resp.status(), StatusCode::OK);

        let _ = std::fs::remove_file(&db_a);
        let _ = std::fs::remove_file(&db_b);
        let _ = std::fs::remove_dir_all(&store_a);
        let _ = std::fs::remove_dir_all(&store_b);
    }

    #[cfg(all(
        feature = "agent-contract-experimental",
        feature = "evolution-network-experimental",
        feature = "sqlite-persistence"
    ))]
    #[tokio::test]
    async fn evolution_a2a_session_rejects_principal_mismatch() {
        let router = build_router(
            ExecutionApiState::with_sqlite_idempotency(build_test_graph().await, ":memory:")
                .with_persisted_api_key_record_with_role(
                    "agent-a",
                    "secret-a",
                    true,
                    ApiRole::Admin,
                )
                .with_persisted_api_key_record_with_role(
                    "agent-b",
                    "secret-b",
                    true,
                    ApiRole::Admin,
                ),
        );

        let handshake_req = Request::builder()
            .method(Method::POST)
            .uri("/v1/evolution/a2a/handshake")
            .header("x-api-key-id", "agent-a")
            .header("x-api-key", "secret-a")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "agent_id": "node-a",
                    "role": "Planner",
                    "capability_level": "A1",
                    "supported_protocols": [
                        {
                            "name": crate::agent_contract::A2A_PROTOCOL_NAME,
                            "version": crate::agent_contract::A2A_PROTOCOL_VERSION
                        }
                    ],
                    "advertised_capabilities": ["EvolutionPublish"]
                })
                .to_string(),
            ))
            .unwrap();
        let handshake_resp = router.clone().oneshot(handshake_req).await.unwrap();
        assert_eq!(handshake_resp.status(), StatusCode::OK);

        let publish_req = Request::builder()
            .method(Method::POST)
            .uri("/v1/evolution/publish")
            .header("x-api-key-id", "agent-b")
            .header("x-api-key", "secret-b")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "sender_id": "node-a",
                    "assets": []
                })
                .to_string(),
            ))
            .unwrap();
        let publish_resp = router.oneshot(publish_req).await.unwrap();
        assert_eq!(publish_resp.status(), StatusCode::FORBIDDEN);
        let publish_body = axum::body::to_bytes(publish_resp.into_body(), usize::MAX)
            .await
            .expect("publish body");
        let publish_json: serde_json::Value =
            serde_json::from_slice(&publish_body).expect("publish json");
        assert_eq!(
            publish_json["error"]["message"],
            "negotiated a2a session principal does not match caller"
        );
    }

    #[cfg(all(
        feature = "agent-contract-experimental",
        feature = "evolution-network-experimental"
    ))]
    #[tokio::test]
    async fn evolution_a2a_handshake_route_accepts_compatible_agent() {
        let router = build_router(ExecutionApiState::new(build_test_graph().await));
        let json =
            handshake_agent_with_caps(&router, "agent-a", &["Coordination", "ReplayFeedback"])
                .await;
        assert_eq!(json["data"]["accepted"], true);
        assert_eq!(
            json["data"]["negotiated_protocol"]["name"],
            crate::agent_contract::A2A_PROTOCOL_NAME
        );
        assert_eq!(
            json["data"]["enabled_capabilities"]
                .as_array()
                .map(Vec::len),
            Some(2)
        );
    }

    #[cfg(all(
        feature = "agent-contract-experimental",
        feature = "evolution-network-experimental"
    ))]
    #[tokio::test]
    async fn evolution_a2a_handshake_route_rejects_incompatible_protocol() {
        let router = build_router(ExecutionApiState::new(build_test_graph().await));
        let req = Request::builder()
            .method(Method::POST)
            .uri("/v1/evolution/a2a/handshake")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "agent_id": "agent-a",
                    "role": "Planner",
                    "capability_level": "A1",
                    "supported_protocols": [
                        { "name": "legacy.a2a", "version": "1.0.0" }
                    ],
                    "advertised_capabilities": ["Coordination"]
                })
                .to_string(),
            ))
            .unwrap();

        let resp = router.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .expect("handshake body");
        let json: serde_json::Value = serde_json::from_slice(&body).expect("handshake json");
        assert_eq!(json["data"]["accepted"], false);
        assert_eq!(
            json["data"]["error"]["code"],
            serde_json::json!("UnsupportedProtocol")
        );
    }

    #[cfg(all(
        feature = "agent-contract-experimental",
        feature = "evolution-network-experimental"
    ))]
    #[tokio::test]
    async fn evolution_a2a_handshake_accepts_v1_protocol_only_client() {
        let router = build_router(ExecutionApiState::new(build_test_graph().await));
        let json = handshake_agent_with_caps_and_protocols(
            &router,
            "/evolution/a2a/hello",
            "agent-v1-only",
            "A3",
            &["Coordination", "ReplayFeedback"],
            &[crate::agent_contract::A2A_PROTOCOL_VERSION_V1],
        )
        .await;
        assert_eq!(json["data"]["accepted"], true);
        assert_eq!(
            json["data"]["negotiated_protocol"]["version"],
            crate::agent_contract::A2A_PROTOCOL_VERSION_V1
        );
    }

    #[cfg(all(
        feature = "agent-contract-experimental",
        feature = "evolution-network-experimental"
    ))]
    #[tokio::test]
    async fn evolution_a2a_namespace_facade_alias_routes_map_to_existing_compat_handlers() {
        let router = build_router(ExecutionApiState::new(build_test_graph().await));
        let handshake = handshake_agent_with_caps_and_protocols(
            &router,
            "/a2a/hello",
            "compat-facade-agent",
            "A4",
            &["Coordination", "SupervisedDevloop", "ReplayFeedback"],
            &[crate::agent_contract::A2A_PROTOCOL_VERSION_V1],
        )
        .await;
        assert_eq!(handshake["data"]["accepted"], true);

        let distribute_req = Request::builder()
            .method(Method::POST)
            .uri("/a2a/tasks/distribute")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "sender_id": "compat-facade-agent",
                    "protocol_version": crate::agent_contract::A2A_PROTOCOL_VERSION_V1,
                    "task_id": "compat-facade-task-1",
                    "task_summary": "compat facade task",
                    "dispatch_id": "dispatch-compat-facade-1",
                    "summary": "compat facade dispatch accepted"
                })
                .to_string(),
            ))
            .unwrap();
        let distribute_resp = router.clone().oneshot(distribute_req).await.unwrap();
        assert_eq!(distribute_resp.status(), StatusCode::OK);
        let distribute_body = axum::body::to_bytes(distribute_resp.into_body(), usize::MAX)
            .await
            .expect("distribute body");
        let distribute_json: serde_json::Value =
            serde_json::from_slice(&distribute_body).expect("distribute json");
        assert_eq!(distribute_json["data"]["state"], "Dispatched");
        let session_id = distribute_json["data"]["session_id"]
            .as_str()
            .expect("session id")
            .to_string();

        let claim_req = Request::builder()
            .method(Method::POST)
            .uri("/a2a/tasks/claim")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "sender_id": "compat-facade-agent",
                    "protocol_version": crate::agent_contract::A2A_PROTOCOL_VERSION_V1
                })
                .to_string(),
            ))
            .unwrap();
        let claim_resp = router.clone().oneshot(claim_req).await.unwrap();
        assert_eq!(claim_resp.status(), StatusCode::OK);
        let claim_body = axum::body::to_bytes(claim_resp.into_body(), usize::MAX)
            .await
            .expect("claim body");
        let claim_json: serde_json::Value =
            serde_json::from_slice(&claim_body).expect("claim json");
        assert_eq!(claim_json["data"]["claimed"], true);
        assert_eq!(
            claim_json["data"]["task"]["task_id"],
            "compat-facade-task-1"
        );

        let complete_req = Request::builder()
            .method(Method::POST)
            .uri("/a2a/tasks/report")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "sender_id": "compat-facade-agent",
                    "protocol_version": crate::agent_contract::A2A_PROTOCOL_VERSION_V1,
                    "task_id": "compat-facade-task-1",
                    "status": "succeeded",
                    "summary": "compat facade task completed",
                    "retryable": false,
                    "retry_after_ms": null,
                    "used_capsule": true,
                    "capsule_id": "compat-facade-capsule-1",
                    "reasoning_steps_avoided": 2,
                    "fallback_reason": null,
                    "task_class_id": "compat.facade",
                    "task_label": "Compat facade task"
                })
                .to_string(),
            ))
            .unwrap();
        let complete_resp = router.clone().oneshot(complete_req).await.unwrap();
        assert_eq!(complete_resp.status(), StatusCode::OK);
        let complete_body = axum::body::to_bytes(complete_resp.into_body(), usize::MAX)
            .await
            .expect("complete body");
        let complete_json: serde_json::Value =
            serde_json::from_slice(&complete_body).expect("complete json");
        assert_eq!(complete_json["data"]["state"], "Completed");
        assert_eq!(complete_json["data"]["terminal_state"], "Succeeded");

        let snapshot_req = Request::builder()
            .method(Method::GET)
            .uri(format!(
                "/v1/evolution/a2a/sessions/{session_id}?sender_id=compat-facade-agent&protocol_version={}",
                crate::agent_contract::A2A_PROTOCOL_VERSION_V1
            ))
            .body(Body::empty())
            .unwrap();
        let snapshot_resp = router.clone().oneshot(snapshot_req).await.unwrap();
        assert_eq!(snapshot_resp.status(), StatusCode::OK);
        let snapshot_body = axum::body::to_bytes(snapshot_resp.into_body(), usize::MAX)
            .await
            .expect("snapshot body");
        let snapshot_json: serde_json::Value =
            serde_json::from_slice(&snapshot_body).expect("snapshot json");
        assert_eq!(snapshot_json["data"]["state"], "Completed");
    }

    #[cfg(all(
        feature = "agent-contract-experimental",
        feature = "evolution-network-experimental"
    ))]
    #[tokio::test]
    async fn evolution_a2a_fetch_include_tasks_supports_claim_flow_discovery() {
        let router = build_router(ExecutionApiState::new(build_test_graph().await));
        let handshake = handshake_agent_with_caps_and_protocols(
            &router,
            "/a2a/hello",
            "compat-fetch-agent",
            "A4",
            &[
                "Coordination",
                "SupervisedDevloop",
                "ReplayFeedback",
                "EvolutionFetch",
            ],
            &[crate::agent_contract::A2A_PROTOCOL_VERSION_V1],
        )
        .await;
        assert_eq!(handshake["data"]["accepted"], true);

        let distribute_req = Request::builder()
            .method(Method::POST)
            .uri("/a2a/tasks/distribute")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "sender_id": "compat-fetch-agent",
                    "protocol_version": crate::agent_contract::A2A_PROTOCOL_VERSION_V1,
                    "task_id": "compat-fetch-task-1",
                    "task_summary": "compat fetch task",
                    "dispatch_id": "dispatch-compat-fetch-1",
                    "summary": "compat fetch dispatch accepted"
                })
                .to_string(),
            ))
            .unwrap();
        let distribute_resp = router.clone().oneshot(distribute_req).await.unwrap();
        assert_eq!(distribute_resp.status(), StatusCode::OK);
        let distribute_body = axum::body::to_bytes(distribute_resp.into_body(), usize::MAX)
            .await
            .expect("distribute body");
        let distribute_json: serde_json::Value =
            serde_json::from_slice(&distribute_body).expect("distribute json");
        let session_id = distribute_json["data"]["session_id"]
            .as_str()
            .expect("session_id");

        let first_fetch_req = Request::builder()
            .method(Method::POST)
            .uri("/a2a/fetch")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "sender_id": "compat-fetch-agent",
                    "protocol_version": crate::agent_contract::A2A_PROTOCOL_VERSION_V1,
                    "signals": ["compat", "fetch"],
                    "include_tasks": true
                })
                .to_string(),
            ))
            .unwrap();
        let first_fetch_resp = router.clone().oneshot(first_fetch_req).await.unwrap();
        assert_eq!(first_fetch_resp.status(), StatusCode::OK);
        let first_fetch_body = axum::body::to_bytes(first_fetch_resp.into_body(), usize::MAX)
            .await
            .expect("first fetch body");
        let first_fetch_json: serde_json::Value =
            serde_json::from_slice(&first_fetch_body).expect("first fetch json");
        assert_eq!(
            first_fetch_json["data"]["tasks"].as_array().map(Vec::len),
            Some(1)
        );
        assert_eq!(
            first_fetch_json["data"]["tasks"][0]["session_id"],
            serde_json::json!(session_id)
        );
        assert_eq!(
            first_fetch_json["data"]["tasks"][0]["task_id"],
            "compat-fetch-task-1"
        );
        assert_eq!(
            first_fetch_json["data"]["tasks"][0]["dispatch_id"],
            "dispatch-compat-fetch-1"
        );
        assert_eq!(first_fetch_json["data"]["tasks"][0]["claimable"], true);

        let claim_req = Request::builder()
            .method(Method::POST)
            .uri("/a2a/tasks/claim")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "sender_id": "compat-fetch-agent",
                    "protocol_version": crate::agent_contract::A2A_PROTOCOL_VERSION_V1
                })
                .to_string(),
            ))
            .unwrap();
        let claim_resp = router.clone().oneshot(claim_req).await.unwrap();
        assert_eq!(claim_resp.status(), StatusCode::OK);

        let second_fetch_req = Request::builder()
            .method(Method::POST)
            .uri("/a2a/fetch")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "sender_id": "compat-fetch-agent",
                    "protocol_version": crate::agent_contract::A2A_PROTOCOL_VERSION_V1,
                    "include_tasks": true
                })
                .to_string(),
            ))
            .unwrap();
        let second_fetch_resp = router.clone().oneshot(second_fetch_req).await.unwrap();
        assert_eq!(second_fetch_resp.status(), StatusCode::OK);
        let second_fetch_body = axum::body::to_bytes(second_fetch_resp.into_body(), usize::MAX)
            .await
            .expect("second fetch body");
        let second_fetch_json: serde_json::Value =
            serde_json::from_slice(&second_fetch_body).expect("second fetch json");
        assert_eq!(
            second_fetch_json["data"]["tasks"].as_array().map(Vec::len),
            Some(1)
        );
        assert_eq!(second_fetch_json["data"]["tasks"][0]["claimable"], false);
        assert!(
            second_fetch_json["data"]["tasks"][0]["lease_expires_at_ms"]
                .as_u64()
                .expect("lease_expires_at_ms should exist after claim")
                > 0
        );
    }

    #[cfg(all(
        feature = "agent-contract-experimental",
        feature = "evolution-network-experimental"
    ))]
    #[tokio::test]
    async fn evolution_a2a_gep_envelope_translation_maps_hello_and_fetch_requests() {
        let router = build_router(ExecutionApiState::new(build_test_graph().await));

        let hello_req = Request::builder()
            .method(Method::POST)
            .uri("/a2a/hello")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "protocol": "gep-a2a",
                    "protocol_version": "1.0.0",
                    "message_type": "hello",
                    "message_id": "msg-gep-hello-1",
                    "sender_id": "gep-envelope-agent",
                    "timestamp": "2026-03-06T00:00:00Z",
                    "payload": {
                        "capabilities": {
                            "coordination": true,
                            "supervised_devloop": true,
                            "replay_feedback": true,
                            "evolution_fetch": true
                        }
                    }
                })
                .to_string(),
            ))
            .unwrap();
        let hello_resp = router.clone().oneshot(hello_req).await.unwrap();
        assert_eq!(hello_resp.status(), StatusCode::OK);
        let hello_body = axum::body::to_bytes(hello_resp.into_body(), usize::MAX)
            .await
            .expect("hello body");
        let hello_json: serde_json::Value =
            serde_json::from_slice(&hello_body).expect("hello json");
        assert_eq!(hello_json["data"]["accepted"], true);
        assert_eq!(
            hello_json["data"]["negotiated_protocol"]["version"],
            crate::agent_contract::A2A_PROTOCOL_VERSION_V1
        );

        let distribute_req = Request::builder()
            .method(Method::POST)
            .uri("/a2a/tasks/distribute")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "sender_id": "gep-envelope-agent",
                    "task_id": "gep-envelope-task-1",
                    "task_summary": "gep envelope task"
                })
                .to_string(),
            ))
            .unwrap();
        let distribute_resp = router.clone().oneshot(distribute_req).await.unwrap();
        assert_eq!(distribute_resp.status(), StatusCode::OK);

        let fetch_req = Request::builder()
            .method(Method::POST)
            .uri("/a2a/fetch")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "protocol": "gep-a2a",
                    "protocol_version": "1.0.0",
                    "message_type": "fetch",
                    "message_id": "msg-gep-fetch-1",
                    "sender_id": "gep-envelope-agent",
                    "timestamp": "2026-03-06T00:00:05Z",
                    "payload": {
                        "signals": ["compat", "gep"],
                        "include_tasks": true
                    }
                })
                .to_string(),
            ))
            .unwrap();
        let fetch_resp = router.clone().oneshot(fetch_req).await.unwrap();
        assert_eq!(fetch_resp.status(), StatusCode::OK);
        let fetch_body = axum::body::to_bytes(fetch_resp.into_body(), usize::MAX)
            .await
            .expect("fetch body");
        let fetch_json: serde_json::Value =
            serde_json::from_slice(&fetch_body).expect("fetch json");
        assert_eq!(
            fetch_json["data"]["tasks"].as_array().map(Vec::len),
            Some(1)
        );
        assert_eq!(
            fetch_json["data"]["tasks"][0]["task_id"],
            "gep-envelope-task-1"
        );
    }

    #[cfg(all(
        feature = "agent-contract-experimental",
        feature = "evolution-network-experimental"
    ))]
    #[tokio::test]
    async fn evolution_a2a_gep_envelope_translation_maps_task_claim_and_complete_requests() {
        let router = build_router(ExecutionApiState::new(build_test_graph().await));
        let handshake = handshake_agent_with_caps_and_protocols(
            &router,
            "/a2a/hello",
            "gep-task-flow-agent",
            "A4",
            &["Coordination", "SupervisedDevloop", "ReplayFeedback"],
            &[crate::agent_contract::A2A_PROTOCOL_VERSION_V1],
        )
        .await;
        assert_eq!(handshake["data"]["accepted"], true);

        let distribute_req = Request::builder()
            .method(Method::POST)
            .uri("/a2a/tasks/distribute")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "sender_id": "gep-task-flow-agent",
                    "task_id": "gep-task-flow-1",
                    "task_summary": "gep task flow"
                })
                .to_string(),
            ))
            .unwrap();
        let distribute_resp = router.clone().oneshot(distribute_req).await.unwrap();
        assert_eq!(distribute_resp.status(), StatusCode::OK);

        let claim_req = Request::builder()
            .method(Method::POST)
            .uri("/a2a/task/claim")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "protocol": "gep-a2a",
                    "protocol_version": "1.0.0",
                    "message_type": "fetch",
                    "message_id": "msg-gep-claim-1",
                    "sender_id": "gep-task-flow-agent",
                    "timestamp": "2026-03-06T00:01:00Z",
                    "payload": {}
                })
                .to_string(),
            ))
            .unwrap();
        let claim_resp = router.clone().oneshot(claim_req).await.unwrap();
        assert_eq!(claim_resp.status(), StatusCode::OK);
        let claim_body = axum::body::to_bytes(claim_resp.into_body(), usize::MAX)
            .await
            .expect("claim body");
        let claim_json: serde_json::Value =
            serde_json::from_slice(&claim_body).expect("claim json");
        assert_eq!(claim_json["data"]["claimed"], true);

        let complete_req = Request::builder()
            .method(Method::POST)
            .uri("/a2a/task/complete")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "protocol": "gep-a2a",
                    "protocol_version": "1.0.0",
                    "message_type": "report",
                    "message_id": "msg-gep-complete-1",
                    "sender_id": "gep-task-flow-agent",
                    "timestamp": "2026-03-06T00:01:30Z",
                    "payload": {
                        "task_id": "gep-task-flow-1",
                        "success": true,
                        "summary": "gep envelope complete succeeded",
                        "used_capsule": true,
                        "capsule_id": "gep-task-flow-capsule-1",
                        "reasoning_steps_avoided": 1
                    }
                })
                .to_string(),
            ))
            .unwrap();
        let complete_resp = router.clone().oneshot(complete_req).await.unwrap();
        assert_eq!(complete_resp.status(), StatusCode::OK);
        let complete_body = axum::body::to_bytes(complete_resp.into_body(), usize::MAX)
            .await
            .expect("complete body");
        let complete_json: serde_json::Value =
            serde_json::from_slice(&complete_body).expect("complete json");
        assert_eq!(complete_json["data"]["state"], "Completed");
        assert_eq!(complete_json["data"]["terminal_state"], "Succeeded");
    }

    #[cfg(all(
        feature = "agent-contract-experimental",
        feature = "evolution-network-experimental"
    ))]
    #[tokio::test]
    async fn evolution_a2a_task_claim_endpoint_reuses_lease_conflict_semantics() {
        let router = build_router(ExecutionApiState::new(build_test_graph().await));
        let handshake = handshake_agent_with_caps_and_protocols(
            &router,
            "/a2a/hello",
            "compat-task-claim-agent",
            "A4",
            &["Coordination", "SupervisedDevloop", "ReplayFeedback"],
            &[crate::agent_contract::A2A_PROTOCOL_VERSION_V1],
        )
        .await;
        assert_eq!(handshake["data"]["accepted"], true);

        let distribute_req = Request::builder()
            .method(Method::POST)
            .uri("/a2a/tasks/distribute")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "node_id": "compat-task-claim-agent",
                    "task_id": "compat-task-claim-1",
                    "task_summary": "compat task claim endpoint",
                    "dispatch_id": "dispatch-compat-task-claim-1",
                    "summary": "compat task claim dispatch accepted"
                })
                .to_string(),
            ))
            .unwrap();
        let distribute_resp = router.clone().oneshot(distribute_req).await.unwrap();
        assert_eq!(distribute_resp.status(), StatusCode::OK);

        let claim_req = Request::builder()
            .method(Method::POST)
            .uri("/a2a/task/claim")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "node_id": "compat-task-claim-agent"
                })
                .to_string(),
            ))
            .unwrap();
        let claim_resp = router.clone().oneshot(claim_req).await.unwrap();
        assert_eq!(claim_resp.status(), StatusCode::OK);
        let claim_body = axum::body::to_bytes(claim_resp.into_body(), usize::MAX)
            .await
            .expect("claim body");
        let claim_json: serde_json::Value =
            serde_json::from_slice(&claim_body).expect("claim json");
        assert_eq!(claim_json["data"]["claimed"], true);
        assert_eq!(claim_json["data"]["task"]["task_id"], "compat-task-claim-1");

        let conflict_claim_req = Request::builder()
            .method(Method::POST)
            .uri("/a2a/task/claim")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "sender_id": "compat-task-claim-agent",
                    "protocol_version": crate::agent_contract::A2A_PROTOCOL_VERSION_V1
                })
                .to_string(),
            ))
            .unwrap();
        let conflict_claim_resp = router.clone().oneshot(conflict_claim_req).await.unwrap();
        assert_eq!(conflict_claim_resp.status(), StatusCode::OK);
        let conflict_claim_body = axum::body::to_bytes(conflict_claim_resp.into_body(), usize::MAX)
            .await
            .expect("conflict claim body");
        let conflict_claim_json: serde_json::Value =
            serde_json::from_slice(&conflict_claim_body).expect("conflict claim json");
        assert_eq!(conflict_claim_json["data"]["claimed"], false);
        assert!(
            conflict_claim_json["data"]["retry_after_ms"]
                .as_u64()
                .expect("retry_after_ms should be present")
                > 0
        );
    }

    #[cfg(all(
        feature = "agent-contract-experimental",
        feature = "evolution-network-experimental"
    ))]
    #[tokio::test]
    async fn evolution_a2a_task_complete_endpoint_maps_terminal_state_and_clears_claimability() {
        let router = build_router(ExecutionApiState::new(build_test_graph().await));
        let handshake = handshake_agent_with_caps_and_protocols(
            &router,
            "/a2a/hello",
            "compat-task-complete-agent",
            "A4",
            &[
                "Coordination",
                "SupervisedDevloop",
                "ReplayFeedback",
                "EvolutionFetch",
            ],
            &[crate::agent_contract::A2A_PROTOCOL_VERSION_V1],
        )
        .await;
        assert_eq!(handshake["data"]["accepted"], true);

        let distribute_req = Request::builder()
            .method(Method::POST)
            .uri("/a2a/tasks/distribute")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "sender_id": "compat-task-complete-agent",
                    "protocol_version": crate::agent_contract::A2A_PROTOCOL_VERSION_V1,
                    "task_id": "compat-task-complete-1",
                    "task_summary": "compat task complete endpoint",
                    "dispatch_id": "dispatch-compat-task-complete-1",
                    "summary": "compat task complete dispatch accepted"
                })
                .to_string(),
            ))
            .unwrap();
        let distribute_resp = router.clone().oneshot(distribute_req).await.unwrap();
        assert_eq!(distribute_resp.status(), StatusCode::OK);

        let claim_req = Request::builder()
            .method(Method::POST)
            .uri("/a2a/task/claim")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "sender_id": "compat-task-complete-agent",
                    "protocol_version": crate::agent_contract::A2A_PROTOCOL_VERSION_V1
                })
                .to_string(),
            ))
            .unwrap();
        let claim_resp = router.clone().oneshot(claim_req).await.unwrap();
        assert_eq!(claim_resp.status(), StatusCode::OK);

        let complete_req = Request::builder()
            .method(Method::POST)
            .uri("/a2a/task/complete")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "node_id": "compat-task-complete-agent",
                    "task_id": "compat-task-complete-1",
                    "success": true,
                    "summary": "compat task complete endpoint succeeded",
                    "used_capsule": true,
                    "capsule_id": "compat-task-complete-capsule-1",
                    "reasoning_steps_avoided": 3,
                    "task_class_id": "compat.task.complete",
                    "task_label": "Compat task complete endpoint"
                })
                .to_string(),
            ))
            .unwrap();
        let complete_resp = router.clone().oneshot(complete_req).await.unwrap();
        assert_eq!(complete_resp.status(), StatusCode::OK);
        let complete_body = axum::body::to_bytes(complete_resp.into_body(), usize::MAX)
            .await
            .expect("complete body");
        let complete_json: serde_json::Value =
            serde_json::from_slice(&complete_body).expect("complete json");
        assert_eq!(complete_json["data"]["state"], "Completed");
        assert_eq!(complete_json["data"]["terminal_state"], "Succeeded");

        let post_complete_claim_req = Request::builder()
            .method(Method::POST)
            .uri("/a2a/task/claim")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "sender_id": "compat-task-complete-agent",
                    "protocol_version": crate::agent_contract::A2A_PROTOCOL_VERSION_V1
                })
                .to_string(),
            ))
            .unwrap();
        let post_complete_claim_resp = router
            .clone()
            .oneshot(post_complete_claim_req)
            .await
            .unwrap();
        assert_eq!(post_complete_claim_resp.status(), StatusCode::OK);
        let post_complete_claim_body =
            axum::body::to_bytes(post_complete_claim_resp.into_body(), usize::MAX)
                .await
                .expect("post-complete claim body");
        let post_complete_claim_json: serde_json::Value =
            serde_json::from_slice(&post_complete_claim_body).expect("post-complete claim json");
        assert_eq!(post_complete_claim_json["data"]["claimed"], false);
        assert!(post_complete_claim_json["data"]["task"].is_null());

        let lifecycle_json = fetch_lifecycle_events(
            &router,
            "compat-task-complete-1",
            "compat-task-complete-agent",
        )
        .await;
        let states = lifecycle_json["data"]["events"]
            .as_array()
            .expect("lifecycle events")
            .iter()
            .filter_map(|event| event["state"].as_str())
            .collect::<Vec<_>>();
        assert!(states.contains(&"Succeeded"));
    }

    #[cfg(all(
        feature = "agent-contract-experimental",
        feature = "evolution-network-experimental"
    ))]
    #[tokio::test]
    async fn evolution_a2a_work_assignment_lifecycle_blocks_double_complete() {
        let router = build_router(ExecutionApiState::new(build_test_graph().await));
        let handshake = handshake_agent_with_caps_and_protocols(
            &router,
            "/a2a/hello",
            "compat-work-agent",
            "A4",
            &["Coordination", "SupervisedDevloop", "ReplayFeedback"],
            &[crate::agent_contract::A2A_PROTOCOL_VERSION_V1],
        )
        .await;
        assert_eq!(handshake["data"]["accepted"], true);

        let distribute_req = Request::builder()
            .method(Method::POST)
            .uri("/a2a/tasks/distribute")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "sender_id": "compat-work-agent",
                    "protocol_version": crate::agent_contract::A2A_PROTOCOL_VERSION_V1,
                    "task_id": "compat-work-task-1",
                    "task_summary": "compat work task",
                    "dispatch_id": "dispatch-compat-work-1",
                    "summary": "compat work dispatch accepted"
                })
                .to_string(),
            ))
            .unwrap();
        let distribute_resp = router.clone().oneshot(distribute_req).await.unwrap();
        assert_eq!(distribute_resp.status(), StatusCode::OK);

        let work_claim_req = Request::builder()
            .method(Method::POST)
            .uri("/a2a/work/claim")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "worker_id": "compat-work-agent"
                })
                .to_string(),
            ))
            .unwrap();
        let work_claim_resp = router.clone().oneshot(work_claim_req).await.unwrap();
        assert_eq!(work_claim_resp.status(), StatusCode::OK);
        let work_claim_body = axum::body::to_bytes(work_claim_resp.into_body(), usize::MAX)
            .await
            .expect("work claim body");
        let work_claim_json: serde_json::Value =
            serde_json::from_slice(&work_claim_body).expect("work claim json");
        assert_eq!(work_claim_json["data"]["claimed"], true);
        let assignment_id = work_claim_json["data"]["assignment"]["assignment_id"]
            .as_str()
            .expect("assignment id")
            .to_string();
        assert_eq!(
            work_claim_json["data"]["assignment"]["task_id"],
            "compat-work-task-1"
        );

        let work_complete_req = Request::builder()
            .method(Method::POST)
            .uri("/a2a/work/complete")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "worker_id": "compat-work-agent",
                    "assignment_id": assignment_id,
                    "task_id": "compat-work-task-1",
                    "success": true,
                    "summary": "compat work complete succeeded",
                    "used_capsule": true,
                    "capsule_id": "compat-work-capsule-1",
                    "reasoning_steps_avoided": 2,
                    "task_class_id": "compat.work",
                    "task_label": "Compat work task"
                })
                .to_string(),
            ))
            .unwrap();
        let work_complete_resp = router.clone().oneshot(work_complete_req).await.unwrap();
        assert_eq!(work_complete_resp.status(), StatusCode::OK);
        let work_complete_body = axum::body::to_bytes(work_complete_resp.into_body(), usize::MAX)
            .await
            .expect("work complete body");
        let work_complete_json: serde_json::Value =
            serde_json::from_slice(&work_complete_body).expect("work complete json");
        assert_eq!(work_complete_json["data"]["assignment_id"], assignment_id);
        assert_eq!(work_complete_json["data"]["state"], "Completed");
        assert_eq!(work_complete_json["data"]["terminal_state"], "Succeeded");

        let duplicate_complete_req = Request::builder()
            .method(Method::POST)
            .uri("/a2a/work/complete")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "worker_id": "compat-work-agent",
                    "assignment_id": assignment_id,
                    "success": true,
                    "summary": "duplicate complete should fail"
                })
                .to_string(),
            ))
            .unwrap();
        let duplicate_complete_resp = router
            .clone()
            .oneshot(duplicate_complete_req)
            .await
            .unwrap();
        assert_eq!(duplicate_complete_resp.status(), StatusCode::CONFLICT);
    }

    #[cfg(all(
        feature = "agent-contract-experimental",
        feature = "evolution-network-experimental"
    ))]
    #[tokio::test]
    async fn evolution_a2a_work_complete_enforces_assignment_ownership() {
        let router = build_router(ExecutionApiState::new(build_test_graph().await));
        let owner_handshake = handshake_agent_with_caps_and_protocols(
            &router,
            "/a2a/hello",
            "compat-work-owner",
            "A4",
            &["Coordination", "SupervisedDevloop", "ReplayFeedback"],
            &[crate::agent_contract::A2A_PROTOCOL_VERSION_V1],
        )
        .await;
        assert_eq!(owner_handshake["data"]["accepted"], true);
        let other_handshake = handshake_agent_with_caps_and_protocols(
            &router,
            "/a2a/hello",
            "compat-work-other",
            "A4",
            &["Coordination", "SupervisedDevloop", "ReplayFeedback"],
            &[crate::agent_contract::A2A_PROTOCOL_VERSION_V1],
        )
        .await;
        assert_eq!(other_handshake["data"]["accepted"], true);

        let distribute_req = Request::builder()
            .method(Method::POST)
            .uri("/a2a/tasks/distribute")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "sender_id": "compat-work-owner",
                    "protocol_version": crate::agent_contract::A2A_PROTOCOL_VERSION_V1,
                    "task_id": "compat-work-owner-task-1",
                    "task_summary": "compat work owner task",
                    "dispatch_id": "dispatch-compat-work-owner-1"
                })
                .to_string(),
            ))
            .unwrap();
        let distribute_resp = router.clone().oneshot(distribute_req).await.unwrap();
        assert_eq!(distribute_resp.status(), StatusCode::OK);

        let work_claim_req = Request::builder()
            .method(Method::POST)
            .uri("/a2a/work/claim")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "sender_id": "compat-work-owner",
                    "protocol_version": crate::agent_contract::A2A_PROTOCOL_VERSION_V1
                })
                .to_string(),
            ))
            .unwrap();
        let work_claim_resp = router.clone().oneshot(work_claim_req).await.unwrap();
        assert_eq!(work_claim_resp.status(), StatusCode::OK);
        let work_claim_body = axum::body::to_bytes(work_claim_resp.into_body(), usize::MAX)
            .await
            .expect("work claim body");
        let work_claim_json: serde_json::Value =
            serde_json::from_slice(&work_claim_body).expect("work claim json");
        let assignment_id = work_claim_json["data"]["assignment"]["assignment_id"]
            .as_str()
            .expect("assignment id");

        let intruder_complete_req = Request::builder()
            .method(Method::POST)
            .uri("/a2a/work/complete")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "sender_id": "compat-work-other",
                    "protocol_version": crate::agent_contract::A2A_PROTOCOL_VERSION_V1,
                    "assignment_id": assignment_id,
                    "success": true,
                    "summary": "intruder should be denied"
                })
                .to_string(),
            ))
            .unwrap();
        let intruder_complete_resp = router.clone().oneshot(intruder_complete_req).await.unwrap();
        assert_eq!(intruder_complete_resp.status(), StatusCode::FORBIDDEN);
    }

    #[cfg(all(
        feature = "agent-contract-experimental",
        feature = "evolution-network-experimental"
    ))]
    #[tokio::test]
    async fn evolution_a2a_heartbeat_reports_deterministic_available_work_shape() {
        let router = build_router(ExecutionApiState::new(build_test_graph().await));
        let handshake = handshake_agent_with_caps_and_protocols(
            &router,
            "/a2a/hello",
            "compat-heartbeat-agent",
            "A4",
            &["Coordination", "SupervisedDevloop", "ReplayFeedback"],
            &[crate::agent_contract::A2A_PROTOCOL_VERSION_V1],
        )
        .await;
        assert_eq!(handshake["data"]["accepted"], true);

        for (task_id, dispatch_id) in [
            ("compat-heartbeat-task-1", "dispatch-heartbeat-1"),
            ("compat-heartbeat-task-2", "dispatch-heartbeat-2"),
        ] {
            let distribute_req = Request::builder()
                .method(Method::POST)
                .uri("/a2a/tasks/distribute")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "sender_id": "compat-heartbeat-agent",
                        "protocol_version": crate::agent_contract::A2A_PROTOCOL_VERSION_V1,
                        "task_id": task_id,
                        "task_summary": format!("summary for {task_id}"),
                        "dispatch_id": dispatch_id
                    })
                    .to_string(),
                ))
                .unwrap();
            let distribute_resp = router.clone().oneshot(distribute_req).await.unwrap();
            assert_eq!(distribute_resp.status(), StatusCode::OK);
        }

        let first_heartbeat_req = Request::builder()
            .method(Method::POST)
            .uri("/a2a/heartbeat")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "worker_id": "compat-heartbeat-agent",
                    "metadata": {
                        "worker_mode": "pull",
                        "version": "test"
                    }
                })
                .to_string(),
            ))
            .unwrap();
        let first_heartbeat_resp = router.clone().oneshot(first_heartbeat_req).await.unwrap();
        assert_eq!(first_heartbeat_resp.status(), StatusCode::OK);
        let first_heartbeat_body =
            axum::body::to_bytes(first_heartbeat_resp.into_body(), usize::MAX)
                .await
                .expect("first heartbeat body");
        let first_heartbeat_json: serde_json::Value =
            serde_json::from_slice(&first_heartbeat_body).expect("first heartbeat json");
        assert_eq!(first_heartbeat_json["data"]["acknowledged"], true);
        assert_eq!(
            first_heartbeat_json["data"]["worker_id"],
            "compat-heartbeat-agent"
        );
        assert_eq!(first_heartbeat_json["data"]["metadata_accepted"], true);
        assert_eq!(first_heartbeat_json["data"]["available_work_count"], 2);
        assert_eq!(
            first_heartbeat_json["data"]["available_work"][0]["task_id"],
            "compat-heartbeat-task-1"
        );
        assert_eq!(
            first_heartbeat_json["data"]["available_work"][1]["task_id"],
            "compat-heartbeat-task-2"
        );

        let claim_req = Request::builder()
            .method(Method::POST)
            .uri("/a2a/work/claim")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "sender_id": "compat-heartbeat-agent",
                    "protocol_version": crate::agent_contract::A2A_PROTOCOL_VERSION_V1
                })
                .to_string(),
            ))
            .unwrap();
        let claim_resp = router.clone().oneshot(claim_req).await.unwrap();
        assert_eq!(claim_resp.status(), StatusCode::OK);

        let second_heartbeat_req = Request::builder()
            .method(Method::POST)
            .uri("/a2a/heartbeat")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "sender_id": "compat-heartbeat-agent",
                    "protocol_version": crate::agent_contract::A2A_PROTOCOL_VERSION_V1
                })
                .to_string(),
            ))
            .unwrap();
        let second_heartbeat_resp = router.clone().oneshot(second_heartbeat_req).await.unwrap();
        assert_eq!(second_heartbeat_resp.status(), StatusCode::OK);
        let second_heartbeat_body =
            axum::body::to_bytes(second_heartbeat_resp.into_body(), usize::MAX)
                .await
                .expect("second heartbeat body");
        let second_heartbeat_json: serde_json::Value =
            serde_json::from_slice(&second_heartbeat_body).expect("second heartbeat json");
        assert_eq!(second_heartbeat_json["data"]["available_work_count"], 1);
        assert_eq!(
            second_heartbeat_json["data"]["available_work"][0]["task_id"],
            "compat-heartbeat-task-2"
        );
    }

    #[cfg(all(
        feature = "agent-contract-experimental",
        feature = "evolution-network-experimental"
    ))]
    #[tokio::test]
    async fn evolution_a2a_fetch_validation_errors_include_a2a_error_code_details() {
        let router = build_router(ExecutionApiState::new(build_test_graph().await));

        let missing_sender_req = Request::builder()
            .method(Method::POST)
            .uri("/a2a/fetch")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "protocol_version": crate::agent_contract::A2A_PROTOCOL_VERSION_V1,
                    "include_tasks": true
                })
                .to_string(),
            ))
            .unwrap();
        let missing_sender_resp = router.clone().oneshot(missing_sender_req).await.unwrap();
        assert_eq!(missing_sender_resp.status(), StatusCode::BAD_REQUEST);
        let missing_sender_body = axum::body::to_bytes(missing_sender_resp.into_body(), usize::MAX)
            .await
            .expect("missing sender body");
        let missing_sender_json: serde_json::Value =
            serde_json::from_slice(&missing_sender_body).expect("missing sender json");
        assert_eq!(
            missing_sender_json["error"]["details"]["a2a_error_code"],
            serde_json::json!("ValidationFailed")
        );

        let unsupported_protocol_req = Request::builder()
            .method(Method::POST)
            .uri("/a2a/fetch")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "sender_id": "compat-fetch-validation-agent",
                    "protocol_version": "0.0.1",
                    "include_tasks": true
                })
                .to_string(),
            ))
            .unwrap();
        let unsupported_protocol_resp = router
            .clone()
            .oneshot(unsupported_protocol_req)
            .await
            .unwrap();
        assert_eq!(unsupported_protocol_resp.status(), StatusCode::BAD_REQUEST);
        let unsupported_protocol_body =
            axum::body::to_bytes(unsupported_protocol_resp.into_body(), usize::MAX)
                .await
                .expect("unsupported protocol body");
        let unsupported_protocol_json: serde_json::Value =
            serde_json::from_slice(&unsupported_protocol_body).expect("unsupported protocol json");
        assert_eq!(
            unsupported_protocol_json["error"]["details"]["a2a_error_code"],
            serde_json::json!("UnsupportedProtocol")
        );

        let wrong_message_type_req = Request::builder()
            .method(Method::POST)
            .uri("/a2a/fetch")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "protocol": "gep-a2a",
                    "protocol_version": "1.0.0",
                    "message_type": "hello",
                    "sender_id": "compat-fetch-validation-agent",
                    "payload": {
                        "include_tasks": true
                    }
                })
                .to_string(),
            ))
            .unwrap();
        let wrong_message_type_resp = router
            .clone()
            .oneshot(wrong_message_type_req)
            .await
            .unwrap();
        assert_eq!(wrong_message_type_resp.status(), StatusCode::BAD_REQUEST);
    }

    #[cfg(not(feature = "evolution-network-experimental"))]
    #[tokio::test]
    async fn evolution_a2a_namespace_facade_routes_remain_feature_gated_when_disabled() {
        let router = build_router(ExecutionApiState::new(build_test_graph().await));
        let req = Request::builder()
            .method(Method::POST)
            .uri("/a2a/hello")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "agent_id": "feature-gate-check-agent",
                    "role": "Planner",
                    "capability_level": "A2",
                    "supported_protocols": [
                        { "name": "oris.a2a", "version": "1.0.0" }
                    ],
                    "advertised_capabilities": ["Coordination"]
                })
                .to_string(),
            ))
            .unwrap();
        let resp = router.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);

        let fetch_req = Request::builder()
            .method(Method::POST)
            .uri("/a2a/fetch")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "sender_id": "feature-gate-check-agent",
                    "include_tasks": true
                })
                .to_string(),
            ))
            .unwrap();
        let fetch_resp = router.oneshot(fetch_req).await.unwrap();
        assert_eq!(fetch_resp.status(), StatusCode::NOT_FOUND);

        let claim_req = Request::builder()
            .method(Method::POST)
            .uri("/a2a/task/claim")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "sender_id": "feature-gate-check-agent"
                })
                .to_string(),
            ))
            .unwrap();
        let claim_resp = router.oneshot(claim_req).await.unwrap();
        assert_eq!(claim_resp.status(), StatusCode::NOT_FOUND);

        let complete_req = Request::builder()
            .method(Method::POST)
            .uri("/a2a/task/complete")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "sender_id": "feature-gate-check-agent",
                    "task_id": "feature-gate-task-1"
                })
                .to_string(),
            ))
            .unwrap();
        let complete_resp = router.oneshot(complete_req).await.unwrap();
        assert_eq!(complete_resp.status(), StatusCode::NOT_FOUND);

        let work_claim_req = Request::builder()
            .method(Method::POST)
            .uri("/a2a/work/claim")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "sender_id": "feature-gate-check-agent"
                })
                .to_string(),
            ))
            .unwrap();
        let work_claim_resp = router.oneshot(work_claim_req).await.unwrap();
        assert_eq!(work_claim_resp.status(), StatusCode::NOT_FOUND);

        let work_complete_req = Request::builder()
            .method(Method::POST)
            .uri("/a2a/work/complete")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "sender_id": "feature-gate-check-agent",
                    "assignment_id": "feature-gate-assignment-1"
                })
                .to_string(),
            ))
            .unwrap();
        let work_complete_resp = router.oneshot(work_complete_req).await.unwrap();
        assert_eq!(work_complete_resp.status(), StatusCode::NOT_FOUND);

        let heartbeat_req = Request::builder()
            .method(Method::POST)
            .uri("/a2a/heartbeat")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "sender_id": "feature-gate-check-agent"
                })
                .to_string(),
            ))
            .unwrap();
        let heartbeat_resp = router.oneshot(heartbeat_req).await.unwrap();
        assert_eq!(heartbeat_resp.status(), StatusCode::NOT_FOUND);
    }

    #[cfg(all(
        feature = "agent-contract-experimental",
        feature = "evolution-network-experimental"
    ))]
    #[tokio::test]
    async fn evolution_a2a_compat_distribute_and_report_map_to_session_flow() {
        let router = build_router(ExecutionApiState::new(build_test_graph().await));
        let handshake = handshake_agent_with_caps_and_protocols(
            &router,
            "/evolution/a2a/hello",
            "compat-agent",
            "A4",
            &["Coordination", "SupervisedDevloop", "ReplayFeedback"],
            &[crate::agent_contract::A2A_PROTOCOL_VERSION_V1],
        )
        .await;
        assert_eq!(handshake["data"]["accepted"], true);

        let distribute_req = Request::builder()
            .method(Method::POST)
            .uri("/evolution/a2a/tasks/distribute")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "sender_id": "compat-agent",
                    "protocol_version": crate::agent_contract::A2A_PROTOCOL_VERSION_V1,
                    "task_id": "compat-task-1",
                    "task_summary": "compat task",
                    "dispatch_id": "dispatch-compat-1",
                    "summary": "compat dispatch accepted"
                })
                .to_string(),
            ))
            .unwrap();
        let distribute_resp = router.clone().oneshot(distribute_req).await.unwrap();
        assert_eq!(distribute_resp.status(), StatusCode::OK);
        let distribute_body = axum::body::to_bytes(distribute_resp.into_body(), usize::MAX)
            .await
            .expect("distribute body");
        let distribute_json: serde_json::Value =
            serde_json::from_slice(&distribute_body).expect("distribute json");
        assert_eq!(distribute_json["data"]["state"], "Dispatched");
        let session_id = distribute_json["data"]["session_id"]
            .as_str()
            .expect("session id")
            .to_string();

        let progress_req = Request::builder()
            .method(Method::POST)
            .uri("/evolution/a2a/tasks/report")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "sender_id": "compat-agent",
                    "protocol_version": crate::agent_contract::A2A_PROTOCOL_VERSION_V1,
                    "task_id": "compat-task-1",
                    "status": "running",
                    "summary": "compat task in progress",
                    "progress_pct": 42,
                    "retryable": false,
                    "retry_after_ms": null
                })
                .to_string(),
            ))
            .unwrap();
        let progress_resp = router.clone().oneshot(progress_req).await.unwrap();
        assert_eq!(progress_resp.status(), StatusCode::OK);
        let progress_body = axum::body::to_bytes(progress_resp.into_body(), usize::MAX)
            .await
            .expect("progress body");
        let progress_json: serde_json::Value =
            serde_json::from_slice(&progress_body).expect("progress json");
        assert_eq!(progress_json["data"]["state"], "InProgress");

        let complete_req = Request::builder()
            .method(Method::POST)
            .uri("/evolution/a2a/tasks/report")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "sender_id": "compat-agent",
                    "protocol_version": crate::agent_contract::A2A_PROTOCOL_VERSION_V1,
                    "task_id": "compat-task-1",
                    "status": "succeeded",
                    "summary": "compat task completed",
                    "retryable": false,
                    "retry_after_ms": null,
                    "used_capsule": true,
                    "capsule_id": "compat-capsule-1",
                    "reasoning_steps_avoided": 3,
                    "fallback_reason": null,
                    "task_class_id": "compat.class",
                    "task_label": "Compat task"
                })
                .to_string(),
            ))
            .unwrap();
        let complete_resp = router.clone().oneshot(complete_req).await.unwrap();
        assert_eq!(complete_resp.status(), StatusCode::OK);
        let complete_body = axum::body::to_bytes(complete_resp.into_body(), usize::MAX)
            .await
            .expect("complete body");
        let complete_json: serde_json::Value =
            serde_json::from_slice(&complete_body).expect("complete json");
        assert_eq!(complete_json["data"]["state"], "Completed");
        assert_eq!(complete_json["data"]["terminal_state"], "Succeeded");

        let snapshot_req = Request::builder()
            .method(Method::GET)
            .uri(format!(
                "/v1/evolution/a2a/sessions/{session_id}?sender_id=compat-agent&protocol_version={}",
                crate::agent_contract::A2A_PROTOCOL_VERSION_V1
            ))
            .body(Body::empty())
            .unwrap();
        let snapshot_resp = router.clone().oneshot(snapshot_req).await.unwrap();
        assert_eq!(snapshot_resp.status(), StatusCode::OK);
        let snapshot_body = axum::body::to_bytes(snapshot_resp.into_body(), usize::MAX)
            .await
            .expect("snapshot body");
        let snapshot_json: serde_json::Value =
            serde_json::from_slice(&snapshot_body).expect("snapshot json");
        assert_eq!(snapshot_json["data"]["state"], "Completed");
    }

    #[cfg(all(
        feature = "agent-contract-experimental",
        feature = "evolution-network-experimental"
    ))]
    #[tokio::test]
    async fn evolution_a2a_compat_claim_returns_distributed_task_and_respects_active_lease() {
        let router = build_router(ExecutionApiState::new(build_test_graph().await));
        let handshake = handshake_agent_with_caps_and_protocols(
            &router,
            "/evolution/a2a/hello",
            "compat-claim-agent",
            "A4",
            &["Coordination", "SupervisedDevloop", "ReplayFeedback"],
            &[crate::agent_contract::A2A_PROTOCOL_VERSION_V1],
        )
        .await;
        assert_eq!(handshake["data"]["accepted"], true);

        let distribute_req = Request::builder()
            .method(Method::POST)
            .uri("/evolution/a2a/tasks/distribute")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "sender_id": "compat-claim-agent",
                    "protocol_version": crate::agent_contract::A2A_PROTOCOL_VERSION_V1,
                    "task_id": "compat-claim-task-1",
                    "task_summary": "compat claim task",
                    "dispatch_id": "dispatch-claim-1",
                    "summary": "dispatch for claim test"
                })
                .to_string(),
            ))
            .unwrap();
        let distribute_resp = router.clone().oneshot(distribute_req).await.unwrap();
        assert_eq!(distribute_resp.status(), StatusCode::OK);

        let claim_req = Request::builder()
            .method(Method::POST)
            .uri("/evolution/a2a/tasks/claim")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "sender_id": "compat-claim-agent",
                    "protocol_version": crate::agent_contract::A2A_PROTOCOL_VERSION_V1
                })
                .to_string(),
            ))
            .unwrap();
        let claim_resp = router.clone().oneshot(claim_req).await.unwrap();
        assert_eq!(claim_resp.status(), StatusCode::OK);
        let claim_body = axum::body::to_bytes(claim_resp.into_body(), usize::MAX)
            .await
            .expect("claim body");
        let claim_json: serde_json::Value =
            serde_json::from_slice(&claim_body).expect("claim json");
        assert_eq!(claim_json["data"]["claimed"], true);
        assert_eq!(claim_json["data"]["task"]["task_id"], "compat-claim-task-1");

        let second_claim_req = Request::builder()
            .method(Method::POST)
            .uri("/evolution/a2a/tasks/claim")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "sender_id": "compat-claim-agent",
                    "protocol_version": crate::agent_contract::A2A_PROTOCOL_VERSION_V1
                })
                .to_string(),
            ))
            .unwrap();
        let second_claim_resp = router.clone().oneshot(second_claim_req).await.unwrap();
        assert_eq!(second_claim_resp.status(), StatusCode::OK);
        let second_claim_body = axum::body::to_bytes(second_claim_resp.into_body(), usize::MAX)
            .await
            .expect("second claim body");
        let second_claim_json: serde_json::Value =
            serde_json::from_slice(&second_claim_body).expect("second claim json");
        assert_eq!(second_claim_json["data"]["claimed"], false);
        assert!(
            second_claim_json["data"]["retry_after_ms"]
                .as_u64()
                .expect("retry_after_ms should be present")
                > 0
        );

        let complete_req = Request::builder()
            .method(Method::POST)
            .uri("/evolution/a2a/tasks/report")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "sender_id": "compat-claim-agent",
                    "protocol_version": crate::agent_contract::A2A_PROTOCOL_VERSION_V1,
                    "task_id": "compat-claim-task-1",
                    "status": "succeeded",
                    "summary": "compat claim task completed",
                    "retryable": false,
                    "retry_after_ms": null,
                    "used_capsule": true,
                    "capsule_id": "compat-claim-capsule-1",
                    "reasoning_steps_avoided": 1,
                    "fallback_reason": null,
                    "task_class_id": "compat.claim",
                    "task_label": "Compat claim task"
                })
                .to_string(),
            ))
            .unwrap();
        let complete_resp = router.clone().oneshot(complete_req).await.unwrap();
        assert_eq!(complete_resp.status(), StatusCode::OK);

        let final_claim_req = Request::builder()
            .method(Method::POST)
            .uri("/evolution/a2a/tasks/claim")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "sender_id": "compat-claim-agent",
                    "protocol_version": crate::agent_contract::A2A_PROTOCOL_VERSION_V1
                })
                .to_string(),
            ))
            .unwrap();
        let final_claim_resp = router.clone().oneshot(final_claim_req).await.unwrap();
        assert_eq!(final_claim_resp.status(), StatusCode::OK);
        let final_claim_body = axum::body::to_bytes(final_claim_resp.into_body(), usize::MAX)
            .await
            .expect("final claim body");
        let final_claim_json: serde_json::Value =
            serde_json::from_slice(&final_claim_body).expect("final claim json");
        assert_eq!(final_claim_json["data"]["claimed"], false);
        assert!(final_claim_json["data"]["task"].is_null());
    }

    #[cfg(all(
        feature = "agent-contract-experimental",
        feature = "evolution-network-experimental"
    ))]
    #[tokio::test]
    async fn evolution_a2a_compat_accepts_node_id_alias_and_defaults_protocol_v1() {
        let router = build_router(ExecutionApiState::new(build_test_graph().await));
        let handshake = handshake_agent_with_caps_and_protocols(
            &router,
            "/evolution/a2a/hello",
            "compat-node-alias-agent",
            "A4",
            &["Coordination", "SupervisedDevloop", "ReplayFeedback"],
            &[crate::agent_contract::A2A_PROTOCOL_VERSION_V1],
        )
        .await;
        assert_eq!(handshake["data"]["accepted"], true);

        let distribute_req = Request::builder()
            .method(Method::POST)
            .uri("/evolution/a2a/tasks/distribute")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "node_id": "compat-node-alias-agent",
                    "task_id": "compat-node-alias-task-1",
                    "task_description": "compat alias task",
                    "dispatch_id": "dispatch-node-alias-1",
                    "summary": "dispatch via node_id alias"
                })
                .to_string(),
            ))
            .unwrap();
        let distribute_resp = router.clone().oneshot(distribute_req).await.unwrap();
        assert_eq!(distribute_resp.status(), StatusCode::OK);

        let claim_req = Request::builder()
            .method(Method::POST)
            .uri("/evolution/a2a/tasks/claim")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "node_id": "compat-node-alias-agent"
                })
                .to_string(),
            ))
            .unwrap();
        let claim_resp = router.clone().oneshot(claim_req).await.unwrap();
        assert_eq!(claim_resp.status(), StatusCode::OK);
        let claim_body = axum::body::to_bytes(claim_resp.into_body(), usize::MAX)
            .await
            .expect("claim body");
        let claim_json: serde_json::Value =
            serde_json::from_slice(&claim_body).expect("claim json");
        assert_eq!(claim_json["data"]["claimed"], true);
        assert_eq!(
            claim_json["data"]["task"]["task_id"],
            "compat-node-alias-task-1"
        );

        let running_req = Request::builder()
            .method(Method::POST)
            .uri("/evolution/a2a/tasks/report")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "node_id": "compat-node-alias-agent",
                    "task_id": "compat-node-alias-task-1",
                    "status": "in_progress",
                    "summary": "compat alias task in progress",
                    "progress_pct": 50,
                    "retryable": false
                })
                .to_string(),
            ))
            .unwrap();
        let running_resp = router.clone().oneshot(running_req).await.unwrap();
        assert_eq!(running_resp.status(), StatusCode::OK);

        let complete_req = Request::builder()
            .method(Method::POST)
            .uri("/evolution/a2a/tasks/report")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "node_id": "compat-node-alias-agent",
                    "task_id": "compat-node-alias-task-1",
                    "status": "completed",
                    "summary": "compat alias task complete",
                    "retryable": false,
                    "used_capsule": true,
                    "capsule_id": "compat-node-alias-capsule-1",
                    "reasoning_steps_avoided": 4,
                    "task_class_id": "compat.alias",
                    "task_label": "Compat alias task"
                })
                .to_string(),
            ))
            .unwrap();
        let complete_resp = router.clone().oneshot(complete_req).await.unwrap();
        assert_eq!(complete_resp.status(), StatusCode::OK);
        let complete_body = axum::body::to_bytes(complete_resp.into_body(), usize::MAX)
            .await
            .expect("complete body");
        let complete_json: serde_json::Value =
            serde_json::from_slice(&complete_body).expect("complete json");
        assert_eq!(complete_json["data"]["state"], "Completed");
        assert_eq!(complete_json["data"]["terminal_state"], "Succeeded");
    }

    #[cfg(all(
        feature = "agent-contract-experimental",
        feature = "evolution-network-experimental"
    ))]
    #[tokio::test]
    async fn evolution_a2a_compat_e2e_fetch_claim_complete_and_heartbeat_supports_route_variants() {
        let router = build_router(ExecutionApiState::new(build_test_graph().await));

        for (idx, hello_path, distribute_path) in [
            (
                1usize,
                "/evolution/a2a/hello",
                "/evolution/a2a/tasks/distribute",
            ),
            (2usize, "/a2a/hello", "/a2a/tasks/distribute"),
        ] {
            let sender_id = format!("compat-e2e-agent-{idx}");
            let task_claim_task_id = format!("compat-e2e-task-claim-{idx}");
            let work_claim_task_id = format!("compat-e2e-work-claim-{idx}");

            let handshake = handshake_agent_with_caps_and_protocols(
                &router,
                hello_path,
                &sender_id,
                "A4",
                &[
                    "Coordination",
                    "SupervisedDevloop",
                    "ReplayFeedback",
                    "EvolutionFetch",
                ],
                &[crate::agent_contract::A2A_PROTOCOL_VERSION_V1],
            )
            .await;
            assert_eq!(handshake["data"]["accepted"], true);

            let distribute_first_req = Request::builder()
                .method(Method::POST)
                .uri(distribute_path)
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "sender_id": sender_id.clone(),
                        "protocol_version": crate::agent_contract::A2A_PROTOCOL_VERSION_V1,
                        "task_id": task_claim_task_id.clone(),
                        "task_summary": "compat e2e task claim flow",
                        "dispatch_id": format!("dispatch-compat-e2e-claim-{idx}"),
                        "summary": "queued for task claim flow"
                    })
                    .to_string(),
                ))
                .unwrap();
            let distribute_first_resp = router.clone().oneshot(distribute_first_req).await.unwrap();
            assert_eq!(distribute_first_resp.status(), StatusCode::OK);

            let distribute_second_req = Request::builder()
                .method(Method::POST)
                .uri(distribute_path)
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "sender_id": sender_id.clone(),
                        "protocol_version": crate::agent_contract::A2A_PROTOCOL_VERSION_V1,
                        "task_id": work_claim_task_id.clone(),
                        "task_summary": "compat e2e work claim flow",
                        "dispatch_id": format!("dispatch-compat-e2e-work-{idx}"),
                        "summary": "queued for work claim flow"
                    })
                    .to_string(),
                ))
                .unwrap();
            let distribute_second_resp =
                router.clone().oneshot(distribute_second_req).await.unwrap();
            assert_eq!(distribute_second_resp.status(), StatusCode::OK);

            let fetch_req = Request::builder()
                .method(Method::POST)
                .uri("/a2a/fetch")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "sender_id": sender_id.clone(),
                        "protocol_version": crate::agent_contract::A2A_PROTOCOL_VERSION_V1,
                        "include_tasks": true
                    })
                    .to_string(),
                ))
                .unwrap();
            let fetch_resp = router.clone().oneshot(fetch_req).await.unwrap();
            assert_eq!(fetch_resp.status(), StatusCode::OK);
            let fetch_body = axum::body::to_bytes(fetch_resp.into_body(), usize::MAX)
                .await
                .expect("fetch body");
            let fetch_json: serde_json::Value =
                serde_json::from_slice(&fetch_body).expect("fetch json");
            let fetched_tasks = fetch_json["data"]["tasks"]
                .as_array()
                .expect("tasks array")
                .iter()
                .filter_map(|task| task["task_id"].as_str())
                .map(ToString::to_string)
                .collect::<std::collections::HashSet<_>>();
            assert!(fetched_tasks.contains(&task_claim_task_id));
            assert!(fetched_tasks.contains(&work_claim_task_id));

            let task_claim_req = Request::builder()
                .method(Method::POST)
                .uri("/a2a/task/claim")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "sender_id": sender_id.clone(),
                        "protocol_version": crate::agent_contract::A2A_PROTOCOL_VERSION_V1
                    })
                    .to_string(),
                ))
                .unwrap();
            let task_claim_resp = router.clone().oneshot(task_claim_req).await.unwrap();
            assert_eq!(task_claim_resp.status(), StatusCode::OK);
            let task_claim_body = axum::body::to_bytes(task_claim_resp.into_body(), usize::MAX)
                .await
                .expect("task claim body");
            let task_claim_json: serde_json::Value =
                serde_json::from_slice(&task_claim_body).expect("task claim json");
            assert_eq!(task_claim_json["data"]["claimed"], true);
            let claimed_task_id = task_claim_json["data"]["task"]["task_id"]
                .as_str()
                .expect("claimed task id")
                .to_string();
            assert!(claimed_task_id == task_claim_task_id || claimed_task_id == work_claim_task_id);

            let task_complete_req = Request::builder()
                .method(Method::POST)
                .uri("/a2a/task/complete")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "sender_id": sender_id.clone(),
                        "protocol_version": crate::agent_contract::A2A_PROTOCOL_VERSION_V1,
                        "task_id": claimed_task_id.clone(),
                        "status": "succeeded",
                        "summary": "compat e2e task complete"
                    })
                    .to_string(),
                ))
                .unwrap();
            let task_complete_resp = router.clone().oneshot(task_complete_req).await.unwrap();
            assert_eq!(task_complete_resp.status(), StatusCode::OK);
            let task_complete_body =
                axum::body::to_bytes(task_complete_resp.into_body(), usize::MAX)
                    .await
                    .expect("task complete body");
            let task_complete_json: serde_json::Value =
                serde_json::from_slice(&task_complete_body).expect("task complete json");
            assert_eq!(task_complete_json["data"]["state"], "Completed");
            assert_eq!(task_complete_json["data"]["terminal_state"], "Succeeded");

            let work_claim_req = Request::builder()
                .method(Method::POST)
                .uri("/a2a/work/claim")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "sender_id": sender_id.clone(),
                        "protocol_version": crate::agent_contract::A2A_PROTOCOL_VERSION_V1
                    })
                    .to_string(),
                ))
                .unwrap();
            let work_claim_resp = router.clone().oneshot(work_claim_req).await.unwrap();
            assert_eq!(work_claim_resp.status(), StatusCode::OK);
            let work_claim_body = axum::body::to_bytes(work_claim_resp.into_body(), usize::MAX)
                .await
                .expect("work claim body");
            let work_claim_json: serde_json::Value =
                serde_json::from_slice(&work_claim_body).expect("work claim json");
            assert_eq!(work_claim_json["data"]["claimed"], true);
            let work_claimed_task_id = work_claim_json["data"]["assignment"]["task_id"]
                .as_str()
                .expect("work claimed task id")
                .to_string();
            assert_ne!(work_claimed_task_id, claimed_task_id);
            let assignment_id = work_claim_json["data"]["assignment"]["assignment_id"]
                .as_str()
                .expect("assignment id")
                .to_string();

            let work_complete_req = Request::builder()
                .method(Method::POST)
                .uri("/a2a/work/complete")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "sender_id": sender_id.clone(),
                        "protocol_version": crate::agent_contract::A2A_PROTOCOL_VERSION_V1,
                        "assignment_id": assignment_id,
                        "task_id": work_claimed_task_id,
                        "status": "succeeded",
                        "summary": "compat e2e work complete"
                    })
                    .to_string(),
                ))
                .unwrap();
            let work_complete_resp = router.clone().oneshot(work_complete_req).await.unwrap();
            assert_eq!(work_complete_resp.status(), StatusCode::OK);
            let work_complete_body =
                axum::body::to_bytes(work_complete_resp.into_body(), usize::MAX)
                    .await
                    .expect("work complete body");
            let work_complete_json: serde_json::Value =
                serde_json::from_slice(&work_complete_body).expect("work complete json");
            assert_eq!(work_complete_json["data"]["state"], "Completed");
            assert_eq!(work_complete_json["data"]["terminal_state"], "Succeeded");

            let heartbeat_req = Request::builder()
                .method(Method::POST)
                .uri("/a2a/heartbeat")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "sender_id": sender_id.clone(),
                        "protocol_version": crate::agent_contract::A2A_PROTOCOL_VERSION_V1,
                        "metadata": {
                            "worker_mode": "compat-e2e"
                        }
                    })
                    .to_string(),
                ))
                .unwrap();
            let heartbeat_resp = router.clone().oneshot(heartbeat_req).await.unwrap();
            assert_eq!(heartbeat_resp.status(), StatusCode::OK);
            let heartbeat_body = axum::body::to_bytes(heartbeat_resp.into_body(), usize::MAX)
                .await
                .expect("heartbeat body");
            let heartbeat_json: serde_json::Value =
                serde_json::from_slice(&heartbeat_body).expect("heartbeat json");
            assert_eq!(heartbeat_json["data"]["acknowledged"], true);
            assert_eq!(heartbeat_json["data"]["metadata_accepted"], true);

            let final_claim_req = Request::builder()
                .method(Method::POST)
                .uri("/a2a/task/claim")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "sender_id": sender_id,
                        "protocol_version": crate::agent_contract::A2A_PROTOCOL_VERSION_V1
                    })
                    .to_string(),
                ))
                .unwrap();
            let final_claim_resp = router.clone().oneshot(final_claim_req).await.unwrap();
            assert_eq!(final_claim_resp.status(), StatusCode::OK);
            let final_claim_body = axum::body::to_bytes(final_claim_resp.into_body(), usize::MAX)
                .await
                .expect("final claim body");
            let final_claim_json: serde_json::Value =
                serde_json::from_slice(&final_claim_body).expect("final claim json");
            assert_eq!(final_claim_json["data"]["claimed"], false);
        }
    }

    #[cfg(all(
        feature = "agent-contract-experimental",
        feature = "evolution-network-experimental"
    ))]
    #[tokio::test]
    async fn evolution_a2a_compat_validation_errors_include_a2a_error_code_details() {
        let router = build_router(ExecutionApiState::new(build_test_graph().await));
        let claim_req = Request::builder()
            .method(Method::POST)
            .uri("/evolution/a2a/tasks/claim")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "protocol_version": crate::agent_contract::A2A_PROTOCOL_VERSION_V1
                })
                .to_string(),
            ))
            .unwrap();
        let claim_resp = router.clone().oneshot(claim_req).await.unwrap();
        assert_eq!(claim_resp.status(), StatusCode::BAD_REQUEST);
        let claim_body = axum::body::to_bytes(claim_resp.into_body(), usize::MAX)
            .await
            .expect("claim body");
        let claim_json: serde_json::Value =
            serde_json::from_slice(&claim_body).expect("claim json");
        assert_eq!(
            claim_json["error"]["details"]["a2a_error_code"],
            serde_json::json!("ValidationFailed")
        );

        let distribute_req = Request::builder()
            .method(Method::POST)
            .uri("/evolution/a2a/tasks/distribute")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "sender_id": "compat-validation-agent",
                    "protocol_version": "0.0.1",
                    "task_id": "compat-validation-task",
                    "task_summary": "compat validation task"
                })
                .to_string(),
            ))
            .unwrap();
        let distribute_resp = router.oneshot(distribute_req).await.unwrap();
        assert_eq!(distribute_resp.status(), StatusCode::BAD_REQUEST);
        let distribute_body = axum::body::to_bytes(distribute_resp.into_body(), usize::MAX)
            .await
            .expect("distribute body");
        let distribute_json: serde_json::Value =
            serde_json::from_slice(&distribute_body).expect("distribute json");
        assert_eq!(
            distribute_json["error"]["details"]["a2a_error_code"],
            serde_json::json!("UnsupportedProtocol")
        );
    }

    #[cfg(all(
        feature = "agent-contract-experimental",
        feature = "evolution-network-experimental",
        feature = "sqlite-persistence"
    ))]
    #[tokio::test]
    async fn evolution_a2a_fetch_include_tasks_reads_sqlite_persistence_queue() {
        let router = build_router(ExecutionApiState::with_sqlite_idempotency(
            build_test_graph().await,
            ":memory:",
        ));
        let handshake = handshake_agent_with_caps_and_protocols(
            &router,
            "/a2a/hello",
            "compat-fetch-sqlite-agent",
            "A4",
            &[
                "Coordination",
                "SupervisedDevloop",
                "ReplayFeedback",
                "EvolutionFetch",
            ],
            &[crate::agent_contract::A2A_PROTOCOL_VERSION_V1],
        )
        .await;
        assert_eq!(handshake["data"]["accepted"], true);

        let distribute_req = Request::builder()
            .method(Method::POST)
            .uri("/a2a/tasks/distribute")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "sender_id": "compat-fetch-sqlite-agent",
                    "protocol_version": crate::agent_contract::A2A_PROTOCOL_VERSION_V1,
                    "task_id": "compat-fetch-sqlite-task-1",
                    "task_summary": "compat fetch sqlite task",
                    "dispatch_id": "dispatch-compat-fetch-sqlite-1",
                    "summary": "compat fetch sqlite dispatch accepted"
                })
                .to_string(),
            ))
            .unwrap();
        let distribute_resp = router.clone().oneshot(distribute_req).await.unwrap();
        assert_eq!(distribute_resp.status(), StatusCode::OK);
        let distribute_body = axum::body::to_bytes(distribute_resp.into_body(), usize::MAX)
            .await
            .expect("distribute body");
        let distribute_json: serde_json::Value =
            serde_json::from_slice(&distribute_body).expect("distribute json");
        let session_id = distribute_json["data"]["session_id"]
            .as_str()
            .expect("session_id")
            .to_string();

        let fetch_req = Request::builder()
            .method(Method::POST)
            .uri("/a2a/fetch")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "sender_id": "compat-fetch-sqlite-agent",
                    "protocol_version": crate::agent_contract::A2A_PROTOCOL_VERSION_V1,
                    "include_tasks": true
                })
                .to_string(),
            ))
            .unwrap();
        let fetch_resp = router.clone().oneshot(fetch_req).await.unwrap();
        assert_eq!(fetch_resp.status(), StatusCode::OK);
        let fetch_body = axum::body::to_bytes(fetch_resp.into_body(), usize::MAX)
            .await
            .expect("fetch body");
        let fetch_json: serde_json::Value =
            serde_json::from_slice(&fetch_body).expect("fetch json");
        assert_eq!(
            fetch_json["data"]["tasks"].as_array().map(Vec::len),
            Some(1)
        );
        assert_eq!(
            fetch_json["data"]["tasks"][0]["session_id"],
            serde_json::json!(session_id)
        );
        assert_eq!(
            fetch_json["data"]["tasks"][0]["task_id"],
            "compat-fetch-sqlite-task-1"
        );
        assert_eq!(fetch_json["data"]["tasks"][0]["claimable"], true);
    }

    #[cfg(all(
        feature = "agent-contract-experimental",
        feature = "evolution-network-experimental",
        feature = "sqlite-persistence"
    ))]
    #[tokio::test]
    async fn evolution_a2a_compat_claim_and_complete_use_sqlite_persistence() {
        let router = build_router(ExecutionApiState::with_sqlite_idempotency(
            build_test_graph().await,
            ":memory:",
        ));
        let handshake = handshake_agent_with_caps_and_protocols(
            &router,
            "/evolution/a2a/hello",
            "compat-sqlite-agent",
            "A4",
            &["Coordination", "SupervisedDevloop", "ReplayFeedback"],
            &[crate::agent_contract::A2A_PROTOCOL_VERSION_V1],
        )
        .await;
        assert_eq!(handshake["data"]["accepted"], true);

        let distribute_req = Request::builder()
            .method(Method::POST)
            .uri("/evolution/a2a/tasks/distribute")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "sender_id": "compat-sqlite-agent",
                    "protocol_version": crate::agent_contract::A2A_PROTOCOL_VERSION_V1,
                    "task_id": "compat-sqlite-task-1",
                    "task_summary": "compat sqlite task",
                    "dispatch_id": "dispatch-sqlite-1",
                    "summary": "dispatch for sqlite claim test"
                })
                .to_string(),
            ))
            .unwrap();
        let distribute_resp = router.clone().oneshot(distribute_req).await.unwrap();
        assert_eq!(distribute_resp.status(), StatusCode::OK);

        let claim_req = Request::builder()
            .method(Method::POST)
            .uri("/evolution/a2a/tasks/claim")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "sender_id": "compat-sqlite-agent",
                    "protocol_version": crate::agent_contract::A2A_PROTOCOL_VERSION_V1
                })
                .to_string(),
            ))
            .unwrap();
        let claim_resp = router.clone().oneshot(claim_req).await.unwrap();
        assert_eq!(claim_resp.status(), StatusCode::OK);
        let claim_body = axum::body::to_bytes(claim_resp.into_body(), usize::MAX)
            .await
            .expect("claim body");
        let claim_json: serde_json::Value =
            serde_json::from_slice(&claim_body).expect("claim json");
        assert_eq!(claim_json["data"]["claimed"], true);
        assert_eq!(
            claim_json["data"]["task"]["task_id"],
            "compat-sqlite-task-1"
        );

        let complete_req = Request::builder()
            .method(Method::POST)
            .uri("/evolution/a2a/tasks/report")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "sender_id": "compat-sqlite-agent",
                    "protocol_version": crate::agent_contract::A2A_PROTOCOL_VERSION_V1,
                    "task_id": "compat-sqlite-task-1",
                    "status": "succeeded",
                    "summary": "compat sqlite task completed",
                    "retryable": false,
                    "retry_after_ms": null,
                    "used_capsule": true,
                    "capsule_id": "compat-sqlite-capsule-1",
                    "reasoning_steps_avoided": 2,
                    "fallback_reason": null,
                    "task_class_id": "compat.sqlite",
                    "task_label": "Compat sqlite task"
                })
                .to_string(),
            ))
            .unwrap();
        let complete_resp = router.clone().oneshot(complete_req).await.unwrap();
        assert_eq!(complete_resp.status(), StatusCode::OK);

        let final_claim_req = Request::builder()
            .method(Method::POST)
            .uri("/evolution/a2a/tasks/claim")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "sender_id": "compat-sqlite-agent",
                    "protocol_version": crate::agent_contract::A2A_PROTOCOL_VERSION_V1
                })
                .to_string(),
            ))
            .unwrap();
        let final_claim_resp = router.clone().oneshot(final_claim_req).await.unwrap();
        assert_eq!(final_claim_resp.status(), StatusCode::OK);
        let final_claim_body = axum::body::to_bytes(final_claim_resp.into_body(), usize::MAX)
            .await
            .expect("final claim body");
        let final_claim_json: serde_json::Value =
            serde_json::from_slice(&final_claim_body).expect("final claim json");
        assert_eq!(final_claim_json["data"]["claimed"], false);
        assert!(final_claim_json["data"]["task"].is_null());
    }

    #[cfg(all(
        feature = "agent-contract-experimental",
        feature = "evolution-network-experimental",
        feature = "sqlite-persistence"
    ))]
    #[tokio::test]
    async fn evolution_a2a_compat_queue_survives_restart_with_sqlite_persistence() {
        let db_path = std::env::temp_dir().join(format!(
            "oris-a2a-compat-queue-{}.sqlite",
            uuid::Uuid::new_v4()
        ));
        let db_path_str = db_path.to_string_lossy().to_string();
        let _ = std::fs::remove_file(&db_path);

        let router = build_router(ExecutionApiState::with_sqlite_idempotency(
            build_test_graph().await,
            &db_path_str,
        ));
        let handshake = handshake_agent_with_caps_and_protocols(
            &router,
            "/evolution/a2a/hello",
            "compat-restart-agent",
            "A4",
            &["Coordination", "SupervisedDevloop", "ReplayFeedback"],
            &[crate::agent_contract::A2A_PROTOCOL_VERSION_V1],
        )
        .await;
        assert_eq!(handshake["data"]["accepted"], true);

        let distribute_req = Request::builder()
            .method(Method::POST)
            .uri("/evolution/a2a/tasks/distribute")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "sender_id": "compat-restart-agent",
                    "protocol_version": crate::agent_contract::A2A_PROTOCOL_VERSION_V1,
                    "task_id": "compat-restart-task-1",
                    "task_summary": "compat restart task",
                    "dispatch_id": "dispatch-restart-1",
                    "summary": "dispatch before restart"
                })
                .to_string(),
            ))
            .unwrap();
        let distribute_resp = router.clone().oneshot(distribute_req).await.unwrap();
        assert_eq!(distribute_resp.status(), StatusCode::OK);
        drop(router);

        let restarted_router = build_router(ExecutionApiState::with_sqlite_idempotency(
            build_test_graph().await,
            &db_path_str,
        ));
        let restarted_handshake = handshake_agent_with_caps_and_protocols(
            &restarted_router,
            "/evolution/a2a/hello",
            "compat-restart-agent",
            "A4",
            &["Coordination", "SupervisedDevloop", "ReplayFeedback"],
            &[crate::agent_contract::A2A_PROTOCOL_VERSION_V1],
        )
        .await;
        assert_eq!(restarted_handshake["data"]["accepted"], true);

        let claim_req = Request::builder()
            .method(Method::POST)
            .uri("/evolution/a2a/tasks/claim")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "sender_id": "compat-restart-agent",
                    "protocol_version": crate::agent_contract::A2A_PROTOCOL_VERSION_V1
                })
                .to_string(),
            ))
            .unwrap();
        let claim_resp = restarted_router.oneshot(claim_req).await.unwrap();
        assert_eq!(claim_resp.status(), StatusCode::OK);
        let claim_body = axum::body::to_bytes(claim_resp.into_body(), usize::MAX)
            .await
            .expect("claim body");
        let claim_json: serde_json::Value =
            serde_json::from_slice(&claim_body).expect("claim json");
        assert_eq!(claim_json["data"]["claimed"], true);
        assert_eq!(
            claim_json["data"]["task"]["task_id"],
            "compat-restart-task-1"
        );

        let _ = std::fs::remove_file(&db_path);
    }

    #[cfg(all(
        feature = "agent-contract-experimental",
        feature = "evolution-network-experimental"
    ))]
    #[tokio::test]
    async fn evolution_a2a_lifecycle_events_track_run_flow_and_query_by_task_id() {
        let router = build_router(ExecutionApiState::new(build_test_graph().await));
        let handshake_json =
            handshake_agent_with_caps(&router, "lifecycle-reader-run", &["EvolutionFetch"]).await;
        assert_eq!(handshake_json["data"]["accepted"], true);

        let run_req = Request::builder()
            .method(Method::POST)
            .uri("/v1/jobs/run")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "thread_id": "a2a-lifecycle-run-1",
                    "input": "run lifecycle"
                })
                .to_string(),
            ))
            .unwrap();
        let run_resp = router.clone().oneshot(run_req).await.unwrap();
        assert_eq!(run_resp.status(), StatusCode::OK);

        let lifecycle_json =
            fetch_lifecycle_events(&router, "a2a-lifecycle-run-1", "lifecycle-reader-run").await;
        let events = lifecycle_json["data"]["events"]
            .as_array()
            .expect("lifecycle events array");
        assert_eq!(events.len(), 3);
        let states: Vec<&str> = events
            .iter()
            .map(|event| {
                event["state"]
                    .as_str()
                    .expect("lifecycle state should be a string")
            })
            .collect();
        assert_eq!(states, vec!["Queued", "Running", "Succeeded"]);

        let updated_at_ms: Vec<u64> = events
            .iter()
            .map(|event| event["updated_at_ms"].as_u64().expect("updated_at_ms"))
            .collect();
        assert!(
            updated_at_ms.windows(2).all(|pair| pair[0] <= pair[1]),
            "lifecycle timestamps should be non-decreasing"
        );
    }

    #[cfg(all(
        feature = "agent-contract-experimental",
        feature = "evolution-network-experimental"
    ))]
    #[tokio::test]
    async fn evolution_a2a_lifecycle_events_capture_replay_failure_terminal_state() {
        let router = build_router(ExecutionApiState::new(build_test_graph().await));
        let handshake_json = handshake_agent_with_caps(
            &router,
            "lifecycle-reader-replay-failure",
            &["EvolutionFetch"],
        )
        .await;
        assert_eq!(handshake_json["data"]["accepted"], true);

        let replay_req = Request::builder()
            .method(Method::POST)
            .uri("/v1/jobs/a2a-replay-missing/replay")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::json!({}).to_string()))
            .unwrap();
        let replay_resp = router.clone().oneshot(replay_req).await.unwrap();
        assert_eq!(replay_resp.status(), StatusCode::INTERNAL_SERVER_ERROR);

        let lifecycle_json = fetch_lifecycle_events(
            &router,
            "a2a-replay-missing",
            "lifecycle-reader-replay-failure",
        )
        .await;
        let events = lifecycle_json["data"]["events"]
            .as_array()
            .expect("lifecycle events array");
        let states: Vec<&str> = events
            .iter()
            .map(|event| event["state"].as_str().expect("lifecycle state"))
            .collect();
        assert_eq!(states, vec!["Running", "Failed"]);
        assert_eq!(events[1]["error"]["code"], serde_json::json!("Internal"));
    }

    #[cfg(all(
        feature = "agent-contract-experimental",
        feature = "evolution-network-experimental",
        feature = "sqlite-persistence"
    ))]
    #[tokio::test]
    async fn evolution_a2a_lifecycle_events_capture_worker_supervised_flow() {
        let state =
            ExecutionApiState::with_sqlite_idempotency(build_test_graph().await, ":memory:");
        let repo = state.runtime_repo.clone().expect("runtime repo");
        repo.enqueue_attempt("attempt-a2a-supervised-1", "run-a2a-supervised-1")
            .expect("enqueue supervised attempt");
        let router = build_router(state);
        let handshake_json =
            handshake_agent_with_caps(&router, "lifecycle-reader-worker", &["EvolutionFetch"])
                .await;
        assert_eq!(handshake_json["data"]["accepted"], true);

        let report_req = Request::builder()
            .method(Method::POST)
            .uri("/v1/workers/worker-a2a/report-step")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "attempt_id": "attempt-a2a-supervised-1",
                    "action_id": "step-1",
                    "status": "running",
                    "dedupe_token": "dedupe-1"
                })
                .to_string(),
            ))
            .unwrap();
        let report_resp = router.clone().oneshot(report_req).await.unwrap();
        assert_eq!(report_resp.status(), StatusCode::OK);

        let ack_req = Request::builder()
            .method(Method::POST)
            .uri("/v1/workers/worker-a2a/ack")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "attempt_id": "attempt-a2a-supervised-1",
                    "terminal_status": "completed"
                })
                .to_string(),
            ))
            .unwrap();
        let ack_resp = router.clone().oneshot(ack_req).await.unwrap();
        assert_eq!(ack_resp.status(), StatusCode::OK);

        let lifecycle_json = fetch_lifecycle_events(
            &router,
            "attempt-a2a-supervised-1",
            "lifecycle-reader-worker",
        )
        .await;
        let events = lifecycle_json["data"]["events"]
            .as_array()
            .expect("lifecycle events array");
        let states: Vec<&str> = events
            .iter()
            .map(|event| event["state"].as_str().expect("lifecycle state"))
            .collect();
        assert_eq!(states, vec!["Running", "Succeeded"]);
    }

    #[cfg(all(
        feature = "agent-contract-experimental",
        feature = "evolution-network-experimental"
    ))]
    #[tokio::test]
    async fn evolution_a2a_remote_task_session_happy_path_is_executable() {
        let router = build_router(ExecutionApiState::new(build_test_graph().await));
        let handshake_json = handshake_agent_with_caps(
            &router,
            "remote-session-agent",
            &["Coordination", "SupervisedDevloop", "ReplayFeedback"],
        )
        .await;
        assert_eq!(handshake_json["data"]["accepted"], true);

        let start_req = Request::builder()
            .method(Method::POST)
            .uri("/v1/evolution/a2a/sessions/start")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "sender_id": "remote-session-agent",
                    "protocol_version": crate::agent_contract::A2A_TASK_SESSION_PROTOCOL_VERSION,
                    "task_id": "remote-a2a-task-1",
                    "task_summary": "remote task session start"
                })
                .to_string(),
            ))
            .unwrap();
        let start_resp = router.clone().oneshot(start_req).await.unwrap();
        assert_eq!(start_resp.status(), StatusCode::OK);
        let start_body = axum::body::to_bytes(start_resp.into_body(), usize::MAX)
            .await
            .expect("start body");
        let start_json: serde_json::Value =
            serde_json::from_slice(&start_body).expect("start json");
        let session_id = start_json["data"]["session_id"]
            .as_str()
            .expect("session_id")
            .to_string();
        assert_eq!(start_json["data"]["state"], "Started");

        let dispatch_req = Request::builder()
            .method(Method::POST)
            .uri(format!("/v1/evolution/a2a/sessions/{session_id}/dispatch"))
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "sender_id": "remote-session-agent",
                    "protocol_version": crate::agent_contract::A2A_TASK_SESSION_PROTOCOL_VERSION,
                    "dispatch_id": "dispatch-1",
                    "summary": "remote dispatch accepted"
                })
                .to_string(),
            ))
            .unwrap();
        let dispatch_resp = router.clone().oneshot(dispatch_req).await.unwrap();
        assert_eq!(dispatch_resp.status(), StatusCode::OK);

        let progress_req = Request::builder()
            .method(Method::POST)
            .uri(format!("/v1/evolution/a2a/sessions/{session_id}/progress"))
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "sender_id": "remote-session-agent",
                    "protocol_version": crate::agent_contract::A2A_TASK_SESSION_PROTOCOL_VERSION,
                    "progress_pct": 60,
                    "summary": "remote execution in progress",
                    "retryable": false,
                    "retry_after_ms": null
                })
                .to_string(),
            ))
            .unwrap();
        let progress_resp = router.clone().oneshot(progress_req).await.unwrap();
        assert_eq!(progress_resp.status(), StatusCode::OK);

        let complete_req = Request::builder()
            .method(Method::POST)
            .uri(format!("/v1/evolution/a2a/sessions/{session_id}/complete"))
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "sender_id": "remote-session-agent",
                    "protocol_version": crate::agent_contract::A2A_TASK_SESSION_PROTOCOL_VERSION,
                    "terminal_state": "Succeeded",
                    "summary": "remote execution completed",
                    "retryable": false,
                    "retry_after_ms": null,
                    "failure_code": null,
                    "failure_details": null,
                    "used_capsule": true,
                    "capsule_id": "capsule-remote-1",
                    "reasoning_steps_avoided": 7,
                    "fallback_reason": null,
                    "task_class_id": "build.fix",
                    "task_label": "Fix build remotely"
                })
                .to_string(),
            ))
            .unwrap();
        let complete_resp = router.clone().oneshot(complete_req).await.unwrap();
        assert_eq!(complete_resp.status(), StatusCode::OK);
        let complete_body = axum::body::to_bytes(complete_resp.into_body(), usize::MAX)
            .await
            .expect("complete body");
        let complete_json: serde_json::Value =
            serde_json::from_slice(&complete_body).expect("complete json");
        assert_eq!(complete_json["data"]["ack"]["state"], "Completed");
        assert_eq!(
            complete_json["data"]["result"]["replay_feedback"]["used_capsule"],
            true
        );
        assert_eq!(
            complete_json["data"]["result"]["replay_feedback"]["planner_directive"],
            "SkipPlanner"
        );
        assert_eq!(
            complete_json["data"]["result"]["replay_feedback"]["reasoning_steps_avoided"],
            7
        );

        let snapshot_req = Request::builder()
            .method(Method::GET)
            .uri(format!(
                "/v1/evolution/a2a/sessions/{session_id}?sender_id=remote-session-agent&protocol_version={}",
                crate::agent_contract::A2A_TASK_SESSION_PROTOCOL_VERSION
            ))
            .body(Body::empty())
            .unwrap();
        let snapshot_resp = router.clone().oneshot(snapshot_req).await.unwrap();
        assert_eq!(snapshot_resp.status(), StatusCode::OK);
        let snapshot_body = axum::body::to_bytes(snapshot_resp.into_body(), usize::MAX)
            .await
            .expect("snapshot body");
        let snapshot_json: serde_json::Value =
            serde_json::from_slice(&snapshot_body).expect("snapshot json");
        assert_eq!(snapshot_json["data"]["state"], "Completed");
        assert_eq!(
            snapshot_json["data"]["dispatch_ids"]
                .as_array()
                .map(Vec::len),
            Some(1)
        );
        assert_eq!(
            snapshot_json["data"]["progress"].as_array().map(Vec::len),
            Some(1)
        );
    }

    #[cfg(all(
        feature = "agent-contract-experimental",
        feature = "evolution-network-experimental"
    ))]
    #[tokio::test]
    async fn evolution_a2a_remote_task_session_failure_semantics_are_deterministic() {
        let router = build_router(ExecutionApiState::new(build_test_graph().await));
        let handshake_json = handshake_agent_with_caps(
            &router,
            "remote-failure-agent",
            &["Coordination", "SupervisedDevloop", "ReplayFeedback"],
        )
        .await;
        assert_eq!(handshake_json["data"]["accepted"], true);

        let start_req = Request::builder()
            .method(Method::POST)
            .uri("/v1/evolution/a2a/sessions/start")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "sender_id": "remote-failure-agent",
                    "protocol_version": crate::agent_contract::A2A_TASK_SESSION_PROTOCOL_VERSION,
                    "task_id": "remote-a2a-task-failed",
                    "task_summary": "remote task session start"
                })
                .to_string(),
            ))
            .unwrap();
        let start_resp = router.clone().oneshot(start_req).await.unwrap();
        assert_eq!(start_resp.status(), StatusCode::OK);
        let start_body = axum::body::to_bytes(start_resp.into_body(), usize::MAX)
            .await
            .expect("start body");
        let start_json: serde_json::Value =
            serde_json::from_slice(&start_body).expect("start json");
        let session_id = start_json["data"]["session_id"]
            .as_str()
            .expect("session_id")
            .to_string();

        let complete_req = Request::builder()
            .method(Method::POST)
            .uri(format!("/v1/evolution/a2a/sessions/{session_id}/complete"))
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "sender_id": "remote-failure-agent",
                    "protocol_version": crate::agent_contract::A2A_TASK_SESSION_PROTOCOL_VERSION,
                    "terminal_state": "Failed",
                    "summary": "remote execution failed",
                    "retryable": true,
                    "retry_after_ms": 120000,
                    "failure_code": "Timeout",
                    "failure_details": "remote worker timeout",
                    "used_capsule": false,
                    "capsule_id": null,
                    "reasoning_steps_avoided": 0,
                    "fallback_reason": "remote timeout fallback",
                    "task_class_id": "build.fix",
                    "task_label": "Fix build remotely"
                })
                .to_string(),
            ))
            .unwrap();
        let complete_resp = router.clone().oneshot(complete_req).await.unwrap();
        assert_eq!(complete_resp.status(), StatusCode::OK);
        let complete_body = axum::body::to_bytes(complete_resp.into_body(), usize::MAX)
            .await
            .expect("complete body");
        let complete_json: serde_json::Value =
            serde_json::from_slice(&complete_body).expect("complete json");
        assert_eq!(complete_json["data"]["ack"]["state"], "Failed");
        assert_eq!(complete_json["data"]["result"]["retryable"], true);
        assert_eq!(complete_json["data"]["result"]["retry_after_ms"], 120000);
        assert_eq!(complete_json["data"]["result"]["failure_code"], "Timeout");
        assert_eq!(
            complete_json["data"]["result"]["replay_feedback"]["planner_directive"],
            "PlanFallback"
        );
    }

    #[cfg(all(
        feature = "agent-contract-experimental",
        feature = "evolution-network-experimental"
    ))]
    #[tokio::test]
    async fn evolution_a2a_remote_task_session_rejects_incompatible_protocol_version() {
        let router = build_router(ExecutionApiState::new(build_test_graph().await));
        let handshake_json =
            handshake_agent_with_caps(&router, "remote-bad-protocol", &["Coordination"]).await;
        assert_eq!(handshake_json["data"]["accepted"], true);

        let start_req = Request::builder()
            .method(Method::POST)
            .uri("/v1/evolution/a2a/sessions/start")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "sender_id": "remote-bad-protocol",
                    "protocol_version": "0.0.1",
                    "task_id": "remote-a2a-task-bad-protocol",
                    "task_summary": "bad protocol"
                })
                .to_string(),
            ))
            .unwrap();
        let start_resp = router.clone().oneshot(start_req).await.unwrap();
        assert_eq!(start_resp.status(), StatusCode::BAD_REQUEST);
        let start_body = axum::body::to_bytes(start_resp.into_body(), usize::MAX)
            .await
            .expect("bad protocol body");
        let start_json: serde_json::Value =
            serde_json::from_slice(&start_body).expect("bad protocol json");
        assert_eq!(
            start_json["error"]["message"],
            "incompatible a2a task session protocol version"
        );
        assert_eq!(
            start_json["error"]["details"]["expected"],
            serde_json::json!([
                crate::agent_contract::A2A_PROTOCOL_VERSION_V1,
                crate::agent_contract::A2A_TASK_SESSION_PROTOCOL_VERSION
            ])
        );
        assert_eq!(start_json["error"]["details"]["actual"], "0.0.1");
    }

    #[cfg(all(
        feature = "agent-contract-experimental",
        feature = "evolution-network-experimental"
    ))]
    #[tokio::test]
    async fn evolution_a2a_privilege_profiles_enforce_allow_deny_matrix() {
        let store_root =
            std::env::temp_dir().join(format!("oris-a2a-privilege-{}", uuid::Uuid::new_v4()));
        let _ = std::fs::remove_dir_all(&store_root);
        let router = build_router(
            ExecutionApiState::new(build_test_graph().await).with_evolution_store(Arc::new(
                crate::evolution::JsonlEvolutionStore::new(&store_root),
            )),
        );

        let full_caps = [
            "Coordination",
            "SupervisedDevloop",
            "ReplayFeedback",
            "EvolutionPublish",
            "EvolutionFetch",
            "EvolutionRevoke",
        ];
        let observer =
            handshake_agent_with_caps_and_level(&router, "profile-observer", "A1", &full_caps)
                .await;
        let operator =
            handshake_agent_with_caps_and_level(&router, "profile-operator", "A3", &full_caps)
                .await;
        let governor =
            handshake_agent_with_caps_and_level(&router, "profile-governor", "A4", &full_caps)
                .await;
        assert_eq!(observer["data"]["accepted"], true);
        assert_eq!(operator["data"]["accepted"], true);
        assert_eq!(governor["data"]["accepted"], true);

        let observer_fetch_req = Request::builder()
            .method(Method::POST)
            .uri("/v1/evolution/fetch")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "sender_id": "profile-observer",
                    "signals": ["rust"]
                })
                .to_string(),
            ))
            .unwrap();
        let observer_fetch_resp = router.clone().oneshot(observer_fetch_req).await.unwrap();
        assert_eq!(observer_fetch_resp.status(), StatusCode::OK);

        let observer_publish_req = Request::builder()
            .method(Method::POST)
            .uri("/v1/evolution/publish")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "sender_id": "profile-observer",
                    "assets": []
                })
                .to_string(),
            ))
            .unwrap();
        let observer_publish_resp = router.clone().oneshot(observer_publish_req).await.unwrap();
        assert_eq!(observer_publish_resp.status(), StatusCode::FORBIDDEN);
        let observer_publish_body =
            axum::body::to_bytes(observer_publish_resp.into_body(), usize::MAX)
                .await
                .expect("observer publish body");
        let observer_publish_json: serde_json::Value =
            serde_json::from_slice(&observer_publish_body).expect("observer publish json");
        assert_eq!(
            observer_publish_json["error"]["message"],
            "a2a privilege profile does not allow this action"
        );

        let operator_publish_req = Request::builder()
            .method(Method::POST)
            .uri("/v1/evolution/publish")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "sender_id": "profile-operator",
                    "assets": []
                })
                .to_string(),
            ))
            .unwrap();
        let operator_publish_resp = router.clone().oneshot(operator_publish_req).await.unwrap();
        assert_eq!(operator_publish_resp.status(), StatusCode::OK);

        let operator_revoke_req = Request::builder()
            .method(Method::POST)
            .uri("/v1/evolution/revoke")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "sender_id": "profile-operator",
                    "asset_ids": [],
                    "reason": "profile test"
                })
                .to_string(),
            ))
            .unwrap();
        let operator_revoke_resp = router.clone().oneshot(operator_revoke_req).await.unwrap();
        assert_eq!(operator_revoke_resp.status(), StatusCode::FORBIDDEN);

        let operator_session_start_req = Request::builder()
            .method(Method::POST)
            .uri("/v1/evolution/a2a/sessions/start")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "sender_id": "profile-operator",
                    "protocol_version": crate::agent_contract::A2A_TASK_SESSION_PROTOCOL_VERSION,
                    "task_id": "profile-operator-task",
                    "task_summary": "operator session start"
                })
                .to_string(),
            ))
            .unwrap();
        let operator_session_start_resp = router
            .clone()
            .oneshot(operator_session_start_req)
            .await
            .unwrap();
        assert_eq!(operator_session_start_resp.status(), StatusCode::OK);

        let governor_revoke_req = Request::builder()
            .method(Method::POST)
            .uri("/v1/evolution/revoke")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "sender_id": "profile-governor",
                    "asset_ids": [],
                    "reason": "governor allowed"
                })
                .to_string(),
            ))
            .unwrap();
        let governor_revoke_resp = router.oneshot(governor_revoke_req).await.unwrap();
        assert_eq!(governor_revoke_resp.status(), StatusCode::OK);

        let _ = std::fs::remove_dir_all(&store_root);
    }

    #[cfg(all(
        feature = "agent-contract-experimental",
        feature = "evolution-network-experimental",
        feature = "sqlite-persistence"
    ))]
    #[tokio::test]
    async fn evolution_a2a_privilege_audit_logs_include_principal_capability_and_reason() {
        let state =
            ExecutionApiState::with_sqlite_idempotency(build_test_graph().await, ":memory:")
                .with_persisted_api_key_record_with_role(
                    "a2a-audit-key",
                    "a2a-audit-secret",
                    true,
                    ApiRole::Admin,
                );
        let repo = state.runtime_repo.clone().expect("runtime repo");
        let router = build_router(state);

        let handshake_req = Request::builder()
            .method(Method::POST)
            .uri("/v1/evolution/a2a/handshake")
            .header("x-api-key-id", "a2a-audit-key")
            .header("x-api-key", "a2a-audit-secret")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "agent_id": "audit-profile-observer",
                    "role": "Planner",
                    "capability_level": "A1",
                    "supported_protocols": [
                        {
                            "name": crate::agent_contract::A2A_PROTOCOL_NAME,
                            "version": crate::agent_contract::A2A_PROTOCOL_VERSION
                        }
                    ],
                    "advertised_capabilities": ["EvolutionPublish", "EvolutionFetch"]
                })
                .to_string(),
            ))
            .unwrap();
        let handshake_resp = router.clone().oneshot(handshake_req).await.unwrap();
        assert_eq!(handshake_resp.status(), StatusCode::OK);

        let denied_req = Request::builder()
            .method(Method::POST)
            .uri("/v1/evolution/publish")
            .header("x-api-key-id", "a2a-audit-key")
            .header("x-api-key", "a2a-audit-secret")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "sender_id": "audit-profile-observer",
                    "assets": []
                })
                .to_string(),
            ))
            .unwrap();
        let denied_resp = router.clone().oneshot(denied_req).await.unwrap();
        assert_eq!(denied_resp.status(), StatusCode::FORBIDDEN);

        let allowed_req = Request::builder()
            .method(Method::POST)
            .uri("/v1/evolution/fetch")
            .header("x-api-key-id", "a2a-audit-key")
            .header("x-api-key", "a2a-audit-secret")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "sender_id": "audit-profile-observer",
                    "signals": ["rust"]
                })
                .to_string(),
            ))
            .unwrap();
        let allowed_resp = router.clone().oneshot(allowed_req).await.unwrap();
        assert_eq!(allowed_resp.status(), StatusCode::OK);

        let logs = repo.list_audit_logs(100).expect("list audit logs");
        let denied_log = logs
            .iter()
            .find(|log| log.action == "a2a.privilege.evolution.publish" && log.result == "denied")
            .expect("denied privilege log");
        let denied_details: serde_json::Value = serde_json::from_str(
            denied_log
                .details_json
                .as_deref()
                .expect("denied details json"),
        )
        .expect("parse denied details");
        assert_eq!(denied_log.actor_id.as_deref(), Some("a2a-audit-key"));
        assert_eq!(denied_details["required_capability"], "EvolutionPublish");
        assert_eq!(denied_details["privilege_profile"], "observer");
        assert_eq!(denied_details["reason"], "profile_denied");

        let allowed_log = logs
            .iter()
            .find(|log| log.action == "a2a.privilege.evolution.fetch" && log.result == "allowed")
            .expect("allowed privilege log");
        let allowed_details: serde_json::Value = serde_json::from_str(
            allowed_log
                .details_json
                .as_deref()
                .expect("allowed details json"),
        )
        .expect("parse allowed details");
        assert_eq!(allowed_details["required_capability"], "EvolutionFetch");
        assert_eq!(allowed_details["reason"], "authorized");
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

    #[cfg(all(
        feature = "agent-contract-experimental",
        feature = "evolution-network-experimental"
    ))]
    #[tokio::test]
    async fn auth_node_secret_compat_mode_only_applies_to_a2a_paths() {
        let router = build_router(
            ExecutionApiState::new(build_test_graph().await)
                .with_compat_node_secret("node-secret-1"),
        );

        let run_req = Request::builder()
            .method(Method::POST)
            .uri("/v1/jobs/run")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "thread_id": "node-secret-auth-scope-1"
                })
                .to_string(),
            ))
            .unwrap();
        let run_resp = router.clone().oneshot(run_req).await.unwrap();
        assert_eq!(run_resp.status(), StatusCode::OK);

        let heartbeat_req = Request::builder()
            .method(Method::POST)
            .uri("/a2a/heartbeat")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "sender_id": "node-secret-auth-scope-1"
                })
                .to_string(),
            ))
            .unwrap();
        let heartbeat_resp = router.clone().oneshot(heartbeat_req).await.unwrap();
        assert_eq!(heartbeat_resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[cfg(all(
        feature = "sqlite-persistence",
        feature = "agent-contract-experimental",
        feature = "evolution-network-experimental"
    ))]
    #[tokio::test]
    async fn auth_node_secret_compat_mode_authenticates_and_audits_denied_calls() {
        let state =
            ExecutionApiState::with_sqlite_idempotency(build_test_graph().await, ":memory:")
                .with_compat_node_secret_with_role("node-secret-1", ApiRole::Operator);
        let repo = state.runtime_repo.clone().expect("runtime repo");
        let router = build_router(state);

        let hello_req = Request::builder()
            .method(Method::POST)
            .uri("/a2a/hello")
            .header("authorization", "Bearer node-secret-1")
            .header("x-request-id", "req-node-secret-hello")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "agent_id": "node-secret-agent",
                    "role": "Planner",
                    "capability_level": "A4",
                    "supported_protocols": [
                        {
                            "name": crate::agent_contract::A2A_PROTOCOL_NAME,
                            "version": crate::agent_contract::A2A_PROTOCOL_VERSION_V1
                        }
                    ],
                    "advertised_capabilities": ["Coordination", "SupervisedDevloop", "ReplayFeedback", "EvolutionFetch"]
                })
                .to_string(),
            ))
            .unwrap();
        let hello_resp = router.clone().oneshot(hello_req).await.unwrap();
        assert_eq!(hello_resp.status(), StatusCode::OK);

        let heartbeat_ok_req = Request::builder()
            .method(Method::POST)
            .uri("/a2a/heartbeat")
            .header("authorization", "Bearer node-secret-1")
            .header("x-request-id", "req-node-secret-ok")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "sender_id": "node-secret-agent",
                    "protocol_version": crate::agent_contract::A2A_PROTOCOL_VERSION_V1
                })
                .to_string(),
            ))
            .unwrap();
        let heartbeat_ok_resp = router.clone().oneshot(heartbeat_ok_req).await.unwrap();
        assert_eq!(heartbeat_ok_resp.status(), StatusCode::OK);

        let heartbeat_denied_req = Request::builder()
            .method(Method::POST)
            .uri("/a2a/heartbeat")
            .header("authorization", "Bearer wrong-node-secret")
            .header("x-request-id", "req-node-secret-denied")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "sender_id": "node-secret-agent",
                    "protocol_version": crate::agent_contract::A2A_PROTOCOL_VERSION_V1
                })
                .to_string(),
            ))
            .unwrap();
        let heartbeat_denied_resp = router.clone().oneshot(heartbeat_denied_req).await.unwrap();
        assert_eq!(heartbeat_denied_resp.status(), StatusCode::UNAUTHORIZED);
        let denied_body = axum::body::to_bytes(heartbeat_denied_resp.into_body(), usize::MAX)
            .await
            .expect("denied body");
        let denied_json: serde_json::Value =
            serde_json::from_slice(&denied_body).expect("denied json");
        let supported_auth = denied_json["error"]["details"]["supported_auth"]
            .as_array()
            .expect("supported auth array");
        assert!(supported_auth
            .iter()
            .any(|value| value.as_str() == Some("bearer(node_secret)")));

        let logs = repo.list_audit_logs(50).expect("list audit logs");
        let denied_log = logs
            .iter()
            .find(|log| {
                log.request_id == "req-node-secret-denied"
                    && log.action == "a2a.compat.heartbeat"
                    && log.result == "error"
            })
            .expect("denied heartbeat audit log");
        let denied_details: serde_json::Value = serde_json::from_str(
            denied_log
                .details_json
                .as_deref()
                .expect("denied heartbeat details json"),
        )
        .expect("parse denied heartbeat details");
        assert_eq!(denied_details["path"], "/a2a/heartbeat");
        assert_eq!(denied_details["status_code"], 401);
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

    #[cfg(all(
        feature = "sqlite-persistence",
        feature = "agent-contract-experimental",
        feature = "evolution-network-experimental"
    ))]
    #[tokio::test]
    async fn audit_logs_capture_a2a_compat_endpoint_actions() {
        let state =
            ExecutionApiState::with_sqlite_idempotency(build_test_graph().await, ":memory:");
        let repo = state.runtime_repo.clone().expect("runtime repo");
        let router = build_router(state);

        let hello_req = Request::builder()
            .method(Method::POST)
            .uri("/evolution/a2a/hello")
            .header("x-request-id", "req-a2a-compat-hello")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "agent_id": "a2a-compat-audit-agent",
                    "role": "Planner",
                    "capability_level": "A4",
                    "supported_protocols": [
                        {
                            "name": crate::agent_contract::A2A_PROTOCOL_NAME,
                            "version": crate::agent_contract::A2A_PROTOCOL_VERSION_V1
                        }
                    ],
                    "advertised_capabilities": ["Coordination", "SupervisedDevloop", "ReplayFeedback", "EvolutionFetch"]
                })
                .to_string(),
            ))
            .unwrap();
        let hello_resp = router.clone().oneshot(hello_req).await.unwrap();
        assert_eq!(hello_resp.status(), StatusCode::OK);

        let distribute_req = Request::builder()
            .method(Method::POST)
            .uri("/evolution/a2a/tasks/distribute")
            .header("x-request-id", "req-a2a-compat-distribute")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "sender_id": "a2a-compat-audit-agent",
                    "protocol_version": crate::agent_contract::A2A_PROTOCOL_VERSION_V1,
                    "task_id": "a2a-compat-audit-task-1",
                    "task_summary": "compat audit task",
                    "dispatch_id": "dispatch-a2a-compat-audit-1",
                    "summary": "compat dispatch"
                })
                .to_string(),
            ))
            .unwrap();
        let distribute_resp = router.clone().oneshot(distribute_req).await.unwrap();
        assert_eq!(distribute_resp.status(), StatusCode::OK);

        let claim_req = Request::builder()
            .method(Method::POST)
            .uri("/evolution/a2a/tasks/claim")
            .header("x-request-id", "req-a2a-compat-claim")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "sender_id": "a2a-compat-audit-agent",
                    "protocol_version": crate::agent_contract::A2A_PROTOCOL_VERSION_V1
                })
                .to_string(),
            ))
            .unwrap();
        let claim_resp = router.clone().oneshot(claim_req).await.unwrap();
        assert_eq!(claim_resp.status(), StatusCode::OK);

        let report_running_req = Request::builder()
            .method(Method::POST)
            .uri("/evolution/a2a/tasks/report")
            .header("x-request-id", "req-a2a-compat-report-running")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "sender_id": "a2a-compat-audit-agent",
                    "protocol_version": crate::agent_contract::A2A_PROTOCOL_VERSION_V1,
                    "task_id": "a2a-compat-audit-task-1",
                    "status": "running",
                    "summary": "compat running",
                    "progress_pct": 40
                })
                .to_string(),
            ))
            .unwrap();
        let report_running_resp = router.clone().oneshot(report_running_req).await.unwrap();
        assert_eq!(report_running_resp.status(), StatusCode::OK);

        let report_complete_req = Request::builder()
            .method(Method::POST)
            .uri("/evolution/a2a/tasks/report")
            .header("x-request-id", "req-a2a-compat-report-complete")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "sender_id": "a2a-compat-audit-agent",
                    "protocol_version": crate::agent_contract::A2A_PROTOCOL_VERSION_V1,
                    "task_id": "a2a-compat-audit-task-1",
                    "status": "succeeded",
                    "summary": "compat complete",
                    "retryable": false,
                    "used_capsule": true,
                    "capsule_id": "a2a-compat-audit-capsule-1",
                    "reasoning_steps_avoided": 1,
                    "task_class_id": "compat.audit",
                    "task_label": "Compat audit"
                })
                .to_string(),
            ))
            .unwrap();
        let report_complete_resp = router.clone().oneshot(report_complete_req).await.unwrap();
        assert_eq!(report_complete_resp.status(), StatusCode::OK);

        let logs = repo.list_audit_logs(200).expect("list audit logs");
        assert!(logs.iter().any(|log| {
            log.action == "a2a.compat.hello"
                && log.result == "success"
                && log.request_id == "req-a2a-compat-hello"
        }));
        assert!(logs.iter().any(|log| {
            log.action == "a2a.compat.distribute"
                && log.result == "success"
                && log.request_id == "req-a2a-compat-distribute"
        }));
        assert!(logs.iter().any(|log| {
            log.action == "a2a.compat.claim"
                && log.result == "success"
                && log.request_id == "req-a2a-compat-claim"
        }));
        assert!(logs.iter().any(|log| {
            log.action == "a2a.compat.report"
                && log.result == "success"
                && log.request_id == "req-a2a-compat-report-running"
        }));
        assert!(logs.iter().any(|log| {
            log.action == "a2a.compat.report"
                && log.result == "success"
                && log.request_id == "req-a2a-compat-report-complete"
        }));
    }

    #[cfg(all(
        feature = "sqlite-persistence",
        feature = "agent-contract-experimental",
        feature = "evolution-network-experimental"
    ))]
    #[tokio::test]
    async fn audit_logs_capture_a2a_compat_fetch_work_heartbeat_actions_with_actor() {
        let state =
            ExecutionApiState::with_sqlite_idempotency(build_test_graph().await, ":memory:")
                .with_compat_node_secret_with_role("compat-audit-secret", ApiRole::Operator);
        let repo = state.runtime_repo.clone().expect("runtime repo");
        let router = build_router(state);
        let node_id = "a2a-compat-node-audit";

        let hello_req = Request::builder()
            .method(Method::POST)
            .uri("/a2a/hello")
            .header("x-request-id", "req-a2a-compat2-hello")
            .header("authorization", "Bearer compat-audit-secret")
            .header("x-node-id", node_id)
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "agent_id": "a2a-compat-audit-agent-2",
                    "role": "Planner",
                    "capability_level": "A4",
                    "supported_protocols": [
                        {
                            "name": crate::agent_contract::A2A_PROTOCOL_NAME,
                            "version": crate::agent_contract::A2A_PROTOCOL_VERSION_V1
                        }
                    ],
                    "advertised_capabilities": ["Coordination", "SupervisedDevloop", "ReplayFeedback", "EvolutionFetch"]
                })
                .to_string(),
            ))
            .unwrap();
        let hello_resp = router.clone().oneshot(hello_req).await.unwrap();
        assert_eq!(hello_resp.status(), StatusCode::OK);

        for (task_id, dispatch_id, request_id) in [
            (
                "a2a-compat-audit-task-2a",
                "dispatch-a2a-compat-audit-2a",
                "req-a2a-compat2-distribute-1",
            ),
            (
                "a2a-compat-audit-task-2b",
                "dispatch-a2a-compat-audit-2b",
                "req-a2a-compat2-distribute-2",
            ),
        ] {
            let distribute_req = Request::builder()
                .method(Method::POST)
                .uri("/a2a/tasks/distribute")
                .header("x-request-id", request_id)
                .header("authorization", "Bearer compat-audit-secret")
                .header("x-node-id", node_id)
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "sender_id": "a2a-compat-audit-agent-2",
                        "protocol_version": crate::agent_contract::A2A_PROTOCOL_VERSION_V1,
                        "task_id": task_id,
                        "task_summary": "compat audit task",
                        "dispatch_id": dispatch_id,
                        "summary": "compat dispatch"
                    })
                    .to_string(),
                ))
                .unwrap();
            let distribute_resp = router.clone().oneshot(distribute_req).await.unwrap();
            assert_eq!(distribute_resp.status(), StatusCode::OK);
        }

        let fetch_req = Request::builder()
            .method(Method::POST)
            .uri("/a2a/fetch")
            .header("x-request-id", "req-a2a-compat2-fetch")
            .header("authorization", "Bearer compat-audit-secret")
            .header("x-node-id", node_id)
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "sender_id": "a2a-compat-audit-agent-2",
                    "protocol_version": crate::agent_contract::A2A_PROTOCOL_VERSION_V1,
                    "include_tasks": true
                })
                .to_string(),
            ))
            .unwrap();
        let fetch_resp = router.clone().oneshot(fetch_req).await.unwrap();
        let fetch_status = fetch_resp.status();
        let fetch_body = axum::body::to_bytes(fetch_resp.into_body(), usize::MAX)
            .await
            .expect("compat fetch body");
        assert_eq!(
            fetch_status,
            StatusCode::OK,
            "{}",
            String::from_utf8_lossy(&fetch_body)
        );

        let task_claim_req = Request::builder()
            .method(Method::POST)
            .uri("/a2a/task/claim")
            .header("x-request-id", "req-a2a-compat2-task-claim")
            .header("authorization", "Bearer compat-audit-secret")
            .header("x-node-id", node_id)
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "sender_id": "a2a-compat-audit-agent-2",
                    "protocol_version": crate::agent_contract::A2A_PROTOCOL_VERSION_V1
                })
                .to_string(),
            ))
            .unwrap();
        let task_claim_resp = router.clone().oneshot(task_claim_req).await.unwrap();
        assert_eq!(task_claim_resp.status(), StatusCode::OK);
        let task_claim_body = axum::body::to_bytes(task_claim_resp.into_body(), usize::MAX)
            .await
            .expect("task claim body");
        let task_claim_json: serde_json::Value =
            serde_json::from_slice(&task_claim_body).expect("task claim json");
        let task_id = task_claim_json["data"]["task"]["task_id"]
            .as_str()
            .expect("task id")
            .to_string();

        let task_complete_req = Request::builder()
            .method(Method::POST)
            .uri("/a2a/task/complete")
            .header("x-request-id", "req-a2a-compat2-task-complete")
            .header("authorization", "Bearer compat-audit-secret")
            .header("x-node-id", node_id)
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "sender_id": "a2a-compat-audit-agent-2",
                    "protocol_version": crate::agent_contract::A2A_PROTOCOL_VERSION_V1,
                    "task_id": task_id,
                    "status": "succeeded",
                    "summary": "compat task complete"
                })
                .to_string(),
            ))
            .unwrap();
        let task_complete_resp = router.clone().oneshot(task_complete_req).await.unwrap();
        assert_eq!(task_complete_resp.status(), StatusCode::OK);

        let work_claim_req = Request::builder()
            .method(Method::POST)
            .uri("/a2a/work/claim")
            .header("x-request-id", "req-a2a-compat2-work-claim")
            .header("authorization", "Bearer compat-audit-secret")
            .header("x-node-id", node_id)
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "sender_id": "a2a-compat-audit-agent-2",
                    "protocol_version": crate::agent_contract::A2A_PROTOCOL_VERSION_V1
                })
                .to_string(),
            ))
            .unwrap();
        let work_claim_resp = router.clone().oneshot(work_claim_req).await.unwrap();
        assert_eq!(work_claim_resp.status(), StatusCode::OK);
        let work_claim_body = axum::body::to_bytes(work_claim_resp.into_body(), usize::MAX)
            .await
            .expect("work claim body");
        let work_claim_json: serde_json::Value =
            serde_json::from_slice(&work_claim_body).expect("work claim json");
        let assignment_id = work_claim_json["data"]["assignment"]["assignment_id"]
            .as_str()
            .expect("assignment id")
            .to_string();

        let work_complete_req = Request::builder()
            .method(Method::POST)
            .uri("/a2a/work/complete")
            .header("x-request-id", "req-a2a-compat2-work-complete")
            .header("authorization", "Bearer compat-audit-secret")
            .header("x-node-id", node_id)
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "sender_id": "a2a-compat-audit-agent-2",
                    "protocol_version": crate::agent_contract::A2A_PROTOCOL_VERSION_V1,
                    "assignment_id": assignment_id,
                    "status": "succeeded",
                    "summary": "compat work complete"
                })
                .to_string(),
            ))
            .unwrap();
        let work_complete_resp = router.clone().oneshot(work_complete_req).await.unwrap();
        assert_eq!(work_complete_resp.status(), StatusCode::OK);

        let heartbeat_req = Request::builder()
            .method(Method::POST)
            .uri("/a2a/heartbeat")
            .header("x-request-id", "req-a2a-compat2-heartbeat")
            .header("authorization", "Bearer compat-audit-secret")
            .header("x-node-id", node_id)
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "sender_id": "a2a-compat-audit-agent-2",
                    "protocol_version": crate::agent_contract::A2A_PROTOCOL_VERSION_V1
                })
                .to_string(),
            ))
            .unwrap();
        let heartbeat_resp = router.clone().oneshot(heartbeat_req).await.unwrap();
        assert_eq!(heartbeat_resp.status(), StatusCode::OK);

        let logs = repo.list_audit_logs(500).expect("list audit logs");
        for (request_id, action) in [
            ("req-a2a-compat2-hello", "a2a.compat.hello"),
            ("req-a2a-compat2-fetch", "a2a.compat.fetch"),
            ("req-a2a-compat2-task-claim", "a2a.compat.claim"),
            ("req-a2a-compat2-task-complete", "a2a.compat.report"),
            ("req-a2a-compat2-work-claim", "a2a.compat.work.claim"),
            ("req-a2a-compat2-work-complete", "a2a.compat.work.complete"),
            ("req-a2a-compat2-heartbeat", "a2a.compat.heartbeat"),
        ] {
            assert!(logs.iter().any(|log| {
                log.request_id == request_id
                    && log.action == action
                    && log.result == "success"
                    && log.actor_type == "node_secret"
                    && log.actor_role.as_deref() == Some("operator")
                    && log.actor_id.as_deref() == Some(node_id)
            }));
        }
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

        let timeline_req = Request::builder()
            .method(Method::GET)
            .uri("/v1/jobs/trace-run-1/timeline")
            .body(Body::empty())
            .unwrap();
        let timeline_resp = router.clone().oneshot(timeline_req).await.unwrap();
        assert_eq!(timeline_resp.status(), StatusCode::OK);
        let timeline_body = axum::body::to_bytes(timeline_resp.into_body(), usize::MAX)
            .await
            .expect("timeline body");
        let timeline_json: serde_json::Value =
            serde_json::from_slice(&timeline_body).expect("timeline json");
        let timeline_trace = timeline_json["data"]["trace"].clone();
        assert_eq!(
            timeline_trace["trace_id"],
            "0123456789abcdef0123456789abcdef"
        );
        assert_eq!(timeline_trace["span_id"], poll_span_id.as_str());
        let reasoning = timeline_json["data"]["observability"]["reasoning_timeline"]
            .as_array()
            .expect("reasoning timeline");
        assert_eq!(
            timeline_json["data"]["observability"]["replay_cost"].as_u64(),
            Some(1)
        );
        assert!(
            reasoning[0]
                .as_str()
                .expect("reasoning entry")
                .contains("CheckpointSaved#1"),
            "timeline should surface checkpoint-derived reasoning entries"
        );
        let lease_graph = timeline_json["data"]["observability"]["lease_graph"]
            .as_array()
            .expect("lease graph");
        assert_eq!(lease_graph[0][0].as_str(), Some(attempt_id.as_str()));
        assert_eq!(lease_graph[0][1].as_str(), Some("trace-worker-1"));

        let export_req = Request::builder()
            .method(Method::GET)
            .uri("/v1/jobs/trace-run-1/timeline/export")
            .body(Body::empty())
            .unwrap();
        let export_resp = router.clone().oneshot(export_req).await.unwrap();
        assert_eq!(export_resp.status(), StatusCode::OK);
        let export_body = axum::body::to_bytes(export_resp.into_body(), usize::MAX)
            .await
            .expect("export body");
        let export_json: serde_json::Value =
            serde_json::from_slice(&export_body).expect("export json");
        assert_eq!(
            export_json["data"]["trace"]["trace_id"].as_str(),
            Some("0123456789abcdef0123456789abcdef")
        );
        assert_eq!(
            export_json["data"]["trace"]["span_id"].as_str(),
            Some(poll_span_id.as_str())
        );

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
        let metrics_resp = router.clone().oneshot(metrics_req).await.unwrap();
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

        let health_req = Request::builder()
            .method(Method::GET)
            .uri("/healthz")
            .body(Body::empty())
            .unwrap();
        let health_resp = router.oneshot(health_req).await.unwrap();
        assert_eq!(health_resp.status(), StatusCode::OK);
        let health_body = axum::body::to_bytes(health_resp.into_body(), usize::MAX)
            .await
            .expect("health body");
        let health_json: serde_json::Value =
            serde_json::from_slice(&health_body).expect("health json");
        assert_eq!(health_json["status"], "ok");
        #[cfg(feature = "evolution-network-experimental")]
        assert_eq!(health_json["evolution"]["status"], "ok");
        #[cfg(not(feature = "evolution-network-experimental"))]
        assert!(health_json["evolution"].is_null());
    }

    #[cfg(all(
        feature = "sqlite-persistence",
        feature = "agent-contract-experimental",
        feature = "evolution-network-experimental"
    ))]
    #[tokio::test]
    async fn metrics_endpoint_exposes_a2a_compat_metrics() {
        let state =
            ExecutionApiState::with_sqlite_idempotency(build_test_graph().await, ":memory:");
        let repo = state.runtime_repo.clone().expect("runtime repo");
        let router = build_router(state);

        let handshake = handshake_agent_with_caps_and_protocols(
            &router,
            "/evolution/a2a/hello",
            "metrics-compat-agent",
            "A4",
            &[
                "Coordination",
                "SupervisedDevloop",
                "ReplayFeedback",
                "EvolutionFetch",
            ],
            &[crate::agent_contract::A2A_PROTOCOL_VERSION_V1],
        )
        .await;
        assert_eq!(handshake["data"]["accepted"], true);

        let distribute_req = Request::builder()
            .method(Method::POST)
            .uri("/evolution/a2a/tasks/distribute")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "sender_id": "metrics-compat-agent",
                    "protocol_version": crate::agent_contract::A2A_PROTOCOL_VERSION_V1,
                    "task_id": "metrics-compat-task-1",
                    "task_summary": "metrics compat task",
                    "dispatch_id": "dispatch-metrics-compat-1",
                    "summary": "metrics compat distribute"
                })
                .to_string(),
            ))
            .unwrap();
        let distribute_resp = router.clone().oneshot(distribute_req).await.unwrap();
        assert_eq!(distribute_resp.status(), StatusCode::OK);
        let distribute_body = axum::body::to_bytes(distribute_resp.into_body(), usize::MAX)
            .await
            .expect("distribute body");
        let distribute_json: serde_json::Value =
            serde_json::from_slice(&distribute_body).expect("distribute json");
        let session_id = distribute_json["data"]["session_id"]
            .as_str()
            .expect("session id")
            .to_string();

        let first_claim_req = Request::builder()
            .method(Method::POST)
            .uri("/evolution/a2a/tasks/claim")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "sender_id": "metrics-compat-agent",
                    "protocol_version": crate::agent_contract::A2A_PROTOCOL_VERSION_V1
                })
                .to_string(),
            ))
            .unwrap();
        let first_claim_resp = router.clone().oneshot(first_claim_req).await.unwrap();
        assert_eq!(first_claim_resp.status(), StatusCode::OK);
        let first_claim_body = axum::body::to_bytes(first_claim_resp.into_body(), usize::MAX)
            .await
            .expect("first claim body");
        let first_claim_json: serde_json::Value =
            serde_json::from_slice(&first_claim_body).expect("first claim json");
        assert_eq!(first_claim_json["data"]["claimed"], true);

        repo.touch_a2a_compat_task_lease(
            &session_id,
            "metrics-compat-agent",
            Utc::now() - Duration::minutes(2),
            1,
        )
        .expect("backdate compat lease to expired");

        let second_claim_req = Request::builder()
            .method(Method::POST)
            .uri("/evolution/a2a/tasks/claim")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "sender_id": "metrics-compat-agent",
                    "protocol_version": crate::agent_contract::A2A_PROTOCOL_VERSION_V1
                })
                .to_string(),
            ))
            .unwrap();
        let second_claim_resp = router.clone().oneshot(second_claim_req).await.unwrap();
        assert_eq!(second_claim_resp.status(), StatusCode::OK);
        let second_claim_body = axum::body::to_bytes(second_claim_resp.into_body(), usize::MAX)
            .await
            .expect("second claim body");
        let second_claim_json: serde_json::Value =
            serde_json::from_slice(&second_claim_body).expect("second claim json");
        assert_eq!(second_claim_json["data"]["claimed"], true);

        let complete_req = Request::builder()
            .method(Method::POST)
            .uri("/evolution/a2a/tasks/report")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "sender_id": "metrics-compat-agent",
                    "protocol_version": crate::agent_contract::A2A_PROTOCOL_VERSION_V1,
                    "task_id": "metrics-compat-task-1",
                    "status": "succeeded",
                    "summary": "metrics compat complete",
                    "retryable": false,
                    "used_capsule": true,
                    "capsule_id": "metrics-compat-capsule-1",
                    "reasoning_steps_avoided": 1,
                    "task_class_id": "compat.metrics",
                    "task_label": "Compat metrics task"
                })
                .to_string(),
            ))
            .unwrap();
        let complete_resp = router.clone().oneshot(complete_req).await.unwrap();
        assert_eq!(complete_resp.status(), StatusCode::OK);

        let compat_distribute_task_2_req = Request::builder()
            .method(Method::POST)
            .uri("/a2a/tasks/distribute")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "sender_id": "metrics-compat-agent",
                    "protocol_version": crate::agent_contract::A2A_PROTOCOL_VERSION_V1,
                    "task_id": "metrics-compat-task-2",
                    "task_summary": "metrics compat task 2",
                    "dispatch_id": "dispatch-metrics-compat-2",
                    "summary": "metrics compat distribute 2"
                })
                .to_string(),
            ))
            .unwrap();
        let compat_distribute_task_2_resp = router
            .clone()
            .oneshot(compat_distribute_task_2_req)
            .await
            .unwrap();
        assert_eq!(compat_distribute_task_2_resp.status(), StatusCode::OK);

        let compat_distribute_task_3_req = Request::builder()
            .method(Method::POST)
            .uri("/a2a/tasks/distribute")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "sender_id": "metrics-compat-agent",
                    "protocol_version": crate::agent_contract::A2A_PROTOCOL_VERSION_V1,
                    "task_id": "metrics-compat-task-3",
                    "task_summary": "metrics compat task 3",
                    "dispatch_id": "dispatch-metrics-compat-3",
                    "summary": "metrics compat distribute 3"
                })
                .to_string(),
            ))
            .unwrap();
        let compat_distribute_task_3_resp = router
            .clone()
            .oneshot(compat_distribute_task_3_req)
            .await
            .unwrap();
        assert_eq!(compat_distribute_task_3_resp.status(), StatusCode::OK);

        let compat_fetch_req = Request::builder()
            .method(Method::POST)
            .uri("/a2a/fetch")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "sender_id": "metrics-compat-agent",
                    "protocol_version": crate::agent_contract::A2A_PROTOCOL_VERSION_V1,
                    "include_tasks": true
                })
                .to_string(),
            ))
            .unwrap();
        let compat_fetch_resp = router.clone().oneshot(compat_fetch_req).await.unwrap();
        assert_eq!(compat_fetch_resp.status(), StatusCode::OK);

        let compat_task_claim_req = Request::builder()
            .method(Method::POST)
            .uri("/a2a/task/claim")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "sender_id": "metrics-compat-agent",
                    "protocol_version": crate::agent_contract::A2A_PROTOCOL_VERSION_V1
                })
                .to_string(),
            ))
            .unwrap();
        let compat_task_claim_resp = router.clone().oneshot(compat_task_claim_req).await.unwrap();
        assert_eq!(compat_task_claim_resp.status(), StatusCode::OK);
        let compat_task_claim_body =
            axum::body::to_bytes(compat_task_claim_resp.into_body(), usize::MAX)
                .await
                .expect("compat task claim body");
        let compat_task_claim_json: serde_json::Value =
            serde_json::from_slice(&compat_task_claim_body).expect("compat task claim json");
        assert_eq!(compat_task_claim_json["data"]["claimed"], true);
        let compat_task_id = compat_task_claim_json["data"]["task"]["task_id"]
            .as_str()
            .expect("compat task id")
            .to_string();

        let compat_task_complete_req = Request::builder()
            .method(Method::POST)
            .uri("/a2a/task/complete")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "sender_id": "metrics-compat-agent",
                    "protocol_version": crate::agent_contract::A2A_PROTOCOL_VERSION_V1,
                    "task_id": compat_task_id,
                    "status": "succeeded",
                    "summary": "metrics compat task complete"
                })
                .to_string(),
            ))
            .unwrap();
        let compat_task_complete_resp = router
            .clone()
            .oneshot(compat_task_complete_req)
            .await
            .unwrap();
        assert_eq!(compat_task_complete_resp.status(), StatusCode::OK);

        let compat_work_claim_req = Request::builder()
            .method(Method::POST)
            .uri("/a2a/work/claim")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "sender_id": "metrics-compat-agent",
                    "protocol_version": crate::agent_contract::A2A_PROTOCOL_VERSION_V1
                })
                .to_string(),
            ))
            .unwrap();
        let compat_work_claim_resp = router.clone().oneshot(compat_work_claim_req).await.unwrap();
        assert_eq!(compat_work_claim_resp.status(), StatusCode::OK);
        let compat_work_claim_body =
            axum::body::to_bytes(compat_work_claim_resp.into_body(), usize::MAX)
                .await
                .expect("compat work claim body");
        let compat_work_claim_json: serde_json::Value =
            serde_json::from_slice(&compat_work_claim_body).expect("compat work claim json");
        assert_eq!(compat_work_claim_json["data"]["claimed"], true);
        let compat_assignment_id = compat_work_claim_json["data"]["assignment"]["assignment_id"]
            .as_str()
            .expect("compat assignment id")
            .to_string();

        let compat_work_complete_req = Request::builder()
            .method(Method::POST)
            .uri("/a2a/work/complete")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "sender_id": "metrics-compat-agent",
                    "protocol_version": crate::agent_contract::A2A_PROTOCOL_VERSION_V1,
                    "assignment_id": compat_assignment_id,
                    "status": "succeeded",
                    "summary": "metrics compat work complete"
                })
                .to_string(),
            ))
            .unwrap();
        let compat_work_complete_resp = router
            .clone()
            .oneshot(compat_work_complete_req)
            .await
            .unwrap();
        assert_eq!(compat_work_complete_resp.status(), StatusCode::OK);

        let compat_heartbeat_req = Request::builder()
            .method(Method::POST)
            .uri("/a2a/heartbeat")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "sender_id": "metrics-compat-agent",
                    "protocol_version": crate::agent_contract::A2A_PROTOCOL_VERSION_V1
                })
                .to_string(),
            ))
            .unwrap();
        let compat_heartbeat_resp = router.clone().oneshot(compat_heartbeat_req).await.unwrap();
        assert_eq!(compat_heartbeat_resp.status(), StatusCode::OK);

        let metrics_req = Request::builder()
            .method(Method::GET)
            .uri("/metrics")
            .body(Body::empty())
            .unwrap();
        let metrics_resp = router.oneshot(metrics_req).await.unwrap();
        assert_eq!(metrics_resp.status(), StatusCode::OK);
        let metrics_body = axum::body::to_bytes(metrics_resp.into_body(), usize::MAX)
            .await
            .expect("metrics body");
        let metrics_text = String::from_utf8(metrics_body.to_vec()).expect("metrics utf8");

        assert!(metrics_text.contains("# HELP oris_a2a_task_queue_depth"));
        assert!(metrics_text.contains("oris_a2a_task_lease_expired_total 1"));
        assert!(metrics_text.contains("oris_a2a_task_claim_latency_ms_count 4"));
        assert!(metrics_text.contains("oris_a2a_report_to_capture_latency_ms_count 3"));
        assert!(metrics_text.contains("oris_a2a_fetch_total 1"));
        assert!(metrics_text.contains("oris_a2a_task_claim_total 1"));
        assert!(metrics_text.contains("oris_a2a_task_complete_total 1"));
        assert!(metrics_text.contains("oris_a2a_work_claim_total 1"));
        assert!(metrics_text.contains("oris_a2a_work_complete_total 1"));
        assert!(metrics_text.contains("oris_a2a_heartbeat_total 1"));
    }

    #[cfg(feature = "evolution-network-experimental")]
    #[tokio::test]
    async fn evolution_metrics_and_health_are_exposed_from_runtime_routes() {
        let store_root = std::env::temp_dir().join(format!(
            "oris-evolution-observability-api-test-{}",
            uuid::Uuid::new_v4()
        ));
        let _ = std::fs::remove_dir_all(&store_root);
        let router = build_router(
            ExecutionApiState::new(build_test_graph().await).with_evolution_store(Arc::new(
                crate::evolution::JsonlEvolutionStore::new(&store_root),
            )),
        );

        let metrics_req = Request::builder()
            .method(Method::GET)
            .uri("/metrics")
            .body(Body::empty())
            .unwrap();
        let metrics_resp = router.clone().oneshot(metrics_req).await.unwrap();
        assert_eq!(metrics_resp.status(), StatusCode::OK);
        let metrics_body = axum::body::to_bytes(metrics_resp.into_body(), usize::MAX)
            .await
            .expect("evolution metrics body");
        let metrics_text =
            String::from_utf8(metrics_body.to_vec()).expect("evolution metrics utf8");
        assert!(metrics_text.contains("# HELP oris_evolution_replay_success_rate"));
        assert!(metrics_text.contains("oris_evolution_replay_success_rate 0.000000"));
        assert!(metrics_text.contains("# HELP oris_evolution_promotion_ratio"));
        assert!(metrics_text.contains("# HELP oris_evolution_revoke_frequency_last_hour"));
        assert!(metrics_text.contains("# HELP oris_evolution_mutation_velocity_last_hour"));
        assert!(metrics_text.contains("oris_evolution_health 1"));

        let health_req = Request::builder()
            .method(Method::GET)
            .uri("/healthz")
            .body(Body::empty())
            .unwrap();
        let health_resp = router.oneshot(health_req).await.unwrap();
        assert_eq!(health_resp.status(), StatusCode::OK);
        let health_body = axum::body::to_bytes(health_resp.into_body(), usize::MAX)
            .await
            .expect("evolution health body");
        let health_json: serde_json::Value =
            serde_json::from_slice(&health_body).expect("evolution health json");
        assert_eq!(health_json["status"], "ok");
        assert_eq!(health_json["evolution"]["status"], "ok");
        assert_eq!(health_json["evolution"]["last_event_seq"], 0);
        let _ = std::fs::remove_dir_all(&store_root);
    }

    #[test]
    fn observability_assets_reference_metrics_present_in_sample_workload() {
        let dashboard = include_str!("../../../../docs/observability/runtime-dashboard.json");
        let alerts = include_str!("../../../../docs/observability/prometheus-alert-rules.yml");
        let sample = include_str!("../../../../docs/observability/sample-runtime-workload.prom");

        let required_metrics = [
            "oris_runtime_queue_depth",
            "oris_runtime_backpressure_total",
            "oris_runtime_dispatch_latency_ms_bucket",
            "oris_runtime_recovery_latency_ms_bucket",
            "oris_runtime_terminal_error_rate",
            "oris_runtime_lease_conflict_rate",
            "oris_a2a_task_queue_depth",
            "oris_a2a_task_claim_latency_ms_bucket",
            "oris_a2a_task_lease_expired_total",
            "oris_a2a_report_to_capture_latency_ms_bucket",
            "oris_a2a_fetch_total",
            "oris_a2a_task_claim_total",
            "oris_a2a_task_complete_total",
            "oris_a2a_work_claim_total",
            "oris_a2a_work_complete_total",
            "oris_a2a_heartbeat_total",
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
            "oris_a2a_task_queue_depth",
            "oris_a2a_task_claim_latency_ms_count",
            "oris_a2a_task_lease_expired_total",
            "oris_a2a_task_complete_total",
            "oris_a2a_work_complete_total",
            "oris_a2a_heartbeat_total",
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
