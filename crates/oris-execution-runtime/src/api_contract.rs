//! Machine-readable runtime API contract generation.

#![cfg(feature = "execution-server")]

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::Serialize;
use serde_json::Value;

use super::api_models::{
    ApiEnvelope, ApiMeta, AttemptRetryHistoryResponse, AuditLogListResponse, CancelJobRequest,
    CancelJobResponse, CheckpointInspectResponse, DeadLetterItem, DeadLetterListResponse,
    DeadLetterReplayResponse, InterruptDetailResponse, InterruptListResponse, JobDetailResponse,
    JobHistoryResponse, JobStateResponse, JobTimelineResponse, ListAuditLogsQuery,
    ListDeadLettersQuery, ListInterruptsQuery, ListJobsQuery, ListJobsResponse,
    RejectInterruptRequest, ReplayJobRequest, ResumeInterruptRequest, ResumeJobRequest,
    RunJobRequest, RunJobResponse, TimelineExportResponse, WorkerAckRequest, WorkerAckResponse,
    WorkerExtendLeaseRequest, WorkerHeartbeatRequest, WorkerLeaseResponse, WorkerPollRequest,
    WorkerPollResponse, WorkerReportStepRequest,
};

pub const RUNTIME_API_CONTRACT_DOC_PATH: &str = "docs/runtime-api-contract.json";

#[derive(Clone, Debug, Serialize)]
pub struct RuntimeApiContract {
    pub api_version: &'static str,
    pub contract_kind: &'static str,
    pub artifact_path: &'static str,
    pub endpoints: Vec<EndpointContract>,
    pub schemas: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Serialize)]
pub struct EndpointContract {
    pub method: &'static str,
    pub path: &'static str,
    pub auth: &'static str,
    pub summary: &'static str,
    pub request_body_schema: Option<&'static str>,
    pub query_schema: Option<&'static str>,
    pub response_content_type: &'static str,
    pub response_schema: Option<&'static str>,
    pub path_params: Vec<PathParamContract>,
}

#[derive(Clone, Debug, Serialize)]
pub struct PathParamContract {
    pub name: &'static str,
    pub schema_type: &'static str,
    pub required: bool,
}

pub fn generate_runtime_api_contract() -> RuntimeApiContract {
    let mut schemas = BTreeMap::new();

    add_schema::<ApiMeta>(&mut schemas, "ApiMeta");
    add_schema::<RunJobRequest>(&mut schemas, "RunJobRequest");
    add_schema::<ResumeJobRequest>(&mut schemas, "ResumeJobRequest");
    add_schema::<ReplayJobRequest>(&mut schemas, "ReplayJobRequest");
    add_schema::<CancelJobRequest>(&mut schemas, "CancelJobRequest");
    add_schema::<WorkerPollRequest>(&mut schemas, "WorkerPollRequest");
    add_schema::<WorkerHeartbeatRequest>(&mut schemas, "WorkerHeartbeatRequest");
    add_schema::<WorkerExtendLeaseRequest>(&mut schemas, "WorkerExtendLeaseRequest");
    add_schema::<WorkerReportStepRequest>(&mut schemas, "WorkerReportStepRequest");
    add_schema::<WorkerAckRequest>(&mut schemas, "WorkerAckRequest");
    add_schema::<ListJobsQuery>(&mut schemas, "ListJobsQuery");
    add_schema::<ListInterruptsQuery>(&mut schemas, "ListInterruptsQuery");
    add_schema::<ListAuditLogsQuery>(&mut schemas, "ListAuditLogsQuery");
    add_schema::<ListDeadLettersQuery>(&mut schemas, "ListDeadLettersQuery");
    add_schema::<ResumeInterruptRequest>(&mut schemas, "ResumeInterruptRequest");
    add_schema::<RejectInterruptRequest>(&mut schemas, "RejectInterruptRequest");

    add_schema::<ApiEnvelope<ListJobsResponse>>(&mut schemas, "ApiEnvelope_ListJobsResponse");
    add_schema::<ApiEnvelope<RunJobResponse>>(&mut schemas, "ApiEnvelope_RunJobResponse");
    add_schema::<ApiEnvelope<JobStateResponse>>(&mut schemas, "ApiEnvelope_JobStateResponse");
    add_schema::<ApiEnvelope<JobDetailResponse>>(&mut schemas, "ApiEnvelope_JobDetailResponse");
    add_schema::<ApiEnvelope<TimelineExportResponse>>(
        &mut schemas,
        "ApiEnvelope_TimelineExportResponse",
    );
    add_schema::<ApiEnvelope<JobHistoryResponse>>(&mut schemas, "ApiEnvelope_JobHistoryResponse");
    add_schema::<ApiEnvelope<JobTimelineResponse>>(&mut schemas, "ApiEnvelope_JobTimelineResponse");
    add_schema::<ApiEnvelope<CheckpointInspectResponse>>(
        &mut schemas,
        "ApiEnvelope_CheckpointInspectResponse",
    );
    add_schema::<ApiEnvelope<CancelJobResponse>>(&mut schemas, "ApiEnvelope_CancelJobResponse");
    add_schema::<ApiEnvelope<WorkerPollResponse>>(&mut schemas, "ApiEnvelope_WorkerPollResponse");
    add_schema::<ApiEnvelope<WorkerLeaseResponse>>(&mut schemas, "ApiEnvelope_WorkerLeaseResponse");
    add_schema::<ApiEnvelope<WorkerAckResponse>>(&mut schemas, "ApiEnvelope_WorkerAckResponse");
    add_schema::<ApiEnvelope<InterruptListResponse>>(
        &mut schemas,
        "ApiEnvelope_InterruptListResponse",
    );
    add_schema::<ApiEnvelope<InterruptDetailResponse>>(
        &mut schemas,
        "ApiEnvelope_InterruptDetailResponse",
    );
    add_schema::<ApiEnvelope<AuditLogListResponse>>(
        &mut schemas,
        "ApiEnvelope_AuditLogListResponse",
    );
    add_schema::<ApiEnvelope<AttemptRetryHistoryResponse>>(
        &mut schemas,
        "ApiEnvelope_AttemptRetryHistoryResponse",
    );
    add_schema::<ApiEnvelope<DeadLetterListResponse>>(
        &mut schemas,
        "ApiEnvelope_DeadLetterListResponse",
    );
    add_schema::<ApiEnvelope<DeadLetterItem>>(&mut schemas, "ApiEnvelope_DeadLetterItem");
    add_schema::<ApiEnvelope<DeadLetterReplayResponse>>(
        &mut schemas,
        "ApiEnvelope_DeadLetterReplayResponse",
    );

    RuntimeApiContract {
        api_version: "v1",
        contract_kind: "json-schema-catalog",
        artifact_path: RUNTIME_API_CONTRACT_DOC_PATH,
        endpoints: vec![
            endpoint(
                "GET",
                "/metrics",
                "public",
                "Prometheus metrics scrape",
                None,
                None,
                "text/plain; version=0.0.4",
                None,
                vec![],
            ),
            endpoint(
                "POST",
                "/a2a/hello",
                "a2a-auth",
                "Negotiate Oris A2A protocol compatibility and capabilities",
                Some("A2aHandshakeRequest"),
                None,
                "application/json",
                Some("ApiEnvelope_A2aHandshakeResponse"),
                vec![],
            ),
            endpoint(
                "POST",
                "/a2a/fetch",
                "a2a-auth",
                "Fetch evolution assets and optional queued tasks for a peer",
                Some("A2aCompatFetchRequest"),
                None,
                "application/json",
                Some("ApiEnvelope_A2aCompatFetchResponse"),
                vec![],
            ),
            endpoint(
                "POST",
                "/a2a/tasks/distribute",
                "a2a-auth",
                "Start a remote task session for distributed work",
                Some("A2aCompatDistributeRequest"),
                None,
                "application/json",
                Some("ApiEnvelope_A2aCompatDistributeResponse"),
                vec![],
            ),
            endpoint(
                "POST",
                "/a2a/task/claim",
                "a2a-auth",
                "Claim a queued task session dispatch for execution",
                Some("A2aCompatClaimRequest"),
                None,
                "application/json",
                Some("ApiEnvelope_A2aCompatClaimResponse"),
                vec![],
            ),
            endpoint(
                "POST",
                "/a2a/task/complete",
                "a2a-auth",
                "Complete a claimed task session dispatch",
                Some("A2aCompatTaskCompleteRequest"),
                None,
                "application/json",
                Some("ApiEnvelope_A2aCompatReportResponse"),
                vec![],
            ),
            endpoint(
                "POST",
                "/a2a/work/claim",
                "a2a-auth",
                "Claim an assignment from the compatibility work queue",
                Some("A2aCompatWorkClaimRequest"),
                None,
                "application/json",
                Some("ApiEnvelope_A2aCompatWorkClaimResponse"),
                vec![],
            ),
            endpoint(
                "POST",
                "/a2a/work/complete",
                "a2a-auth",
                "Complete an assignment claimed from the compatibility work queue",
                Some("A2aCompatWorkCompleteRequest"),
                None,
                "application/json",
                Some("ApiEnvelope_A2aCompatWorkCompleteResponse"),
                vec![],
            ),
            endpoint(
                "POST",
                "/a2a/heartbeat",
                "a2a-auth",
                "Declare node liveness and advertise available work",
                Some("A2aCompatHeartbeatRequest"),
                None,
                "application/json",
                Some("ApiEnvelope_A2aCompatHeartbeatResponse"),
                vec![],
            ),
            endpoint(
                "POST",
                "/a2a/publish",
                "a2a-auth",
                "Publish evolution assets into the remote node import pipeline",
                Some("PublishRequest"),
                None,
                "application/json",
                Some("ApiEnvelope_ImportOutcome"),
                vec![],
            ),
            endpoint(
                "POST",
                "/a2a/revoke",
                "a2a-auth",
                "Revoke previously published evolution assets",
                Some("RevokeNotice"),
                None,
                "application/json",
                Some("ApiEnvelope_RevokeNotice"),
                vec![],
            ),
            endpoint(
                "GET",
                "/v1/audit/logs",
                "api-auth",
                "List audit log records",
                None,
                Some("ListAuditLogsQuery"),
                "application/json",
                Some("ApiEnvelope_AuditLogListResponse"),
                vec![],
            ),
            endpoint(
                "GET",
                "/v1/attempts/:attempt_id/retries",
                "api-auth",
                "Inspect retry history for an attempt",
                None,
                None,
                "application/json",
                Some("ApiEnvelope_AttemptRetryHistoryResponse"),
                vec![path_param("attempt_id")],
            ),
            endpoint(
                "GET",
                "/v1/dlq",
                "api-auth",
                "List dead-letter queue entries",
                None,
                Some("ListDeadLettersQuery"),
                "application/json",
                Some("ApiEnvelope_DeadLetterListResponse"),
                vec![],
            ),
            endpoint(
                "GET",
                "/v1/dlq/:attempt_id",
                "api-auth",
                "Inspect a dead-letter queue entry",
                None,
                None,
                "application/json",
                Some("ApiEnvelope_DeadLetterItem"),
                vec![path_param("attempt_id")],
            ),
            endpoint(
                "POST",
                "/v1/dlq/:attempt_id/replay",
                "api-auth",
                "Replay a dead-letter queue entry",
                None,
                None,
                "application/json",
                Some("ApiEnvelope_DeadLetterReplayResponse"),
                vec![path_param("attempt_id")],
            ),
            endpoint(
                "GET",
                "/v1/jobs",
                "api-auth",
                "List jobs",
                None,
                Some("ListJobsQuery"),
                "application/json",
                Some("ApiEnvelope_ListJobsResponse"),
                vec![],
            ),
            endpoint(
                "POST",
                "/v1/jobs",
                "api-auth",
                "Create a job",
                Some("RunJobRequest"),
                None,
                "application/json",
                Some("ApiEnvelope_RunJobResponse"),
                vec![],
            ),
            endpoint(
                "POST",
                "/v1/jobs/run",
                "api-auth",
                "Create a job via explicit run endpoint",
                Some("RunJobRequest"),
                None,
                "application/json",
                Some("ApiEnvelope_RunJobResponse"),
                vec![],
            ),
            endpoint(
                "GET",
                "/v1/jobs/:thread_id",
                "api-auth",
                "Inspect job state",
                None,
                None,
                "application/json",
                Some("ApiEnvelope_JobStateResponse"),
                vec![path_param("thread_id")],
            ),
            endpoint(
                "GET",
                "/v1/jobs/:thread_id/detail",
                "api-auth",
                "Inspect job detail",
                None,
                None,
                "application/json",
                Some("ApiEnvelope_JobDetailResponse"),
                vec![path_param("thread_id")],
            ),
            endpoint(
                "GET",
                "/v1/jobs/:thread_id/timeline/export",
                "api-auth",
                "Export job timeline",
                None,
                None,
                "application/json",
                Some("ApiEnvelope_TimelineExportResponse"),
                vec![path_param("thread_id")],
            ),
            endpoint(
                "GET",
                "/v1/jobs/:thread_id/history",
                "api-auth",
                "List job checkpoints",
                None,
                None,
                "application/json",
                Some("ApiEnvelope_JobHistoryResponse"),
                vec![path_param("thread_id")],
            ),
            endpoint(
                "GET",
                "/v1/jobs/:thread_id/timeline",
                "api-auth",
                "List job timeline events",
                None,
                None,
                "application/json",
                Some("ApiEnvelope_JobTimelineResponse"),
                vec![path_param("thread_id")],
            ),
            endpoint(
                "GET",
                "/v1/jobs/:thread_id/checkpoints/:checkpoint_id",
                "api-auth",
                "Inspect a checkpoint snapshot",
                None,
                None,
                "application/json",
                Some("ApiEnvelope_CheckpointInspectResponse"),
                vec![path_param("thread_id"), path_param("checkpoint_id")],
            ),
            endpoint(
                "POST",
                "/v1/jobs/:thread_id/resume",
                "api-auth",
                "Resume a blocked job",
                Some("ResumeJobRequest"),
                None,
                "application/json",
                Some("ApiEnvelope_RunJobResponse"),
                vec![path_param("thread_id")],
            ),
            endpoint(
                "POST",
                "/v1/jobs/:thread_id/replay",
                "api-auth",
                "Replay a job from the current or selected checkpoint",
                Some("ReplayJobRequest"),
                None,
                "application/json",
                Some("ApiEnvelope_RunJobResponse"),
                vec![path_param("thread_id")],
            ),
            endpoint(
                "POST",
                "/v1/jobs/:thread_id/cancel",
                "api-auth",
                "Cancel a job",
                Some("CancelJobRequest"),
                None,
                "application/json",
                Some("ApiEnvelope_CancelJobResponse"),
                vec![path_param("thread_id")],
            ),
            endpoint(
                "POST",
                "/v1/workers/poll",
                "api-auth",
                "Poll for one dispatchable attempt",
                Some("WorkerPollRequest"),
                None,
                "application/json",
                Some("ApiEnvelope_WorkerPollResponse"),
                vec![],
            ),
            endpoint(
                "POST",
                "/v1/workers/:worker_id/heartbeat",
                "api-auth",
                "Refresh a worker lease heartbeat",
                Some("WorkerHeartbeatRequest"),
                None,
                "application/json",
                Some("ApiEnvelope_WorkerLeaseResponse"),
                vec![path_param("worker_id")],
            ),
            endpoint(
                "POST",
                "/v1/workers/:worker_id/extend-lease",
                "api-auth",
                "Extend a worker lease",
                Some("WorkerExtendLeaseRequest"),
                None,
                "application/json",
                Some("ApiEnvelope_WorkerLeaseResponse"),
                vec![path_param("worker_id")],
            ),
            endpoint(
                "POST",
                "/v1/workers/:worker_id/report-step",
                "api-auth",
                "Report step execution progress",
                Some("WorkerReportStepRequest"),
                None,
                "application/json",
                Some("ApiEnvelope_WorkerAckResponse"),
                vec![path_param("worker_id")],
            ),
            endpoint(
                "POST",
                "/v1/workers/:worker_id/ack",
                "api-auth",
                "Acknowledge terminal attempt state",
                Some("WorkerAckRequest"),
                None,
                "application/json",
                Some("ApiEnvelope_WorkerAckResponse"),
                vec![path_param("worker_id")],
            ),
            endpoint(
                "GET",
                "/v1/interrupts",
                "api-auth",
                "List pending and historical interrupts",
                None,
                Some("ListInterruptsQuery"),
                "application/json",
                Some("ApiEnvelope_InterruptListResponse"),
                vec![],
            ),
            endpoint(
                "GET",
                "/v1/interrupts/:interrupt_id",
                "api-auth",
                "Inspect a single interrupt",
                None,
                None,
                "application/json",
                Some("ApiEnvelope_InterruptDetailResponse"),
                vec![path_param("interrupt_id")],
            ),
            endpoint(
                "POST",
                "/v1/interrupts/:interrupt_id/resume",
                "api-auth",
                "Resume an interrupt",
                Some("ResumeInterruptRequest"),
                None,
                "application/json",
                Some("ApiEnvelope_RunJobResponse"),
                vec![path_param("interrupt_id")],
            ),
            endpoint(
                "POST",
                "/v1/interrupts/:interrupt_id/reject",
                "api-auth",
                "Reject an interrupt and cancel the thread",
                Some("RejectInterruptRequest"),
                None,
                "application/json",
                Some("ApiEnvelope_CancelJobResponse"),
                vec![path_param("interrupt_id")],
            ),
        ],
        schemas,
    }
}

pub fn runtime_api_contract_pretty_json() -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(&generate_runtime_api_contract())
}

pub fn write_runtime_api_contract(
    path: impl AsRef<Path>,
) -> Result<(), Box<dyn std::error::Error>> {
    let rendered = runtime_api_contract_pretty_json()?;
    fs::write(path, rendered)?;
    Ok(())
}

pub fn canonical_runtime_api_contract_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../")
        .join(RUNTIME_API_CONTRACT_DOC_PATH)
}

fn add_schema<T: JsonSchema>(schemas: &mut BTreeMap<String, Value>, name: &str) {
    let schema = schemars::schema_for!(T);
    let value = canonicalize_json(serde_json::to_value(&schema).expect("serialize schema"));
    schemas.insert(name.to_string(), value);
}

fn canonicalize_json(value: Value) -> Value {
    match value {
        Value::Array(items) => Value::Array(items.into_iter().map(canonicalize_json).collect()),
        Value::Object(entries) => {
            let mut sorted = BTreeMap::new();
            for (key, value) in entries {
                sorted.insert(key, canonicalize_json(value));
            }
            Value::Object(sorted.into_iter().collect())
        }
        other => other,
    }
}

fn endpoint(
    method: &'static str,
    path: &'static str,
    auth: &'static str,
    summary: &'static str,
    request_body_schema: Option<&'static str>,
    query_schema: Option<&'static str>,
    response_content_type: &'static str,
    response_schema: Option<&'static str>,
    path_params: Vec<PathParamContract>,
) -> EndpointContract {
    EndpointContract {
        method,
        path,
        auth,
        summary,
        request_body_schema,
        query_schema,
        response_content_type,
        response_schema,
        path_params,
    }
}

fn path_param(name: &'static str) -> PathParamContract {
    PathParamContract {
        name,
        schema_type: "string",
        required: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_runtime_api_contract_matches_checked_in_artifact() {
        let generated = runtime_api_contract_pretty_json().expect("render contract");
        let checked_in = fs::read_to_string(canonical_runtime_api_contract_path())
            .expect("read checked-in contract");
        assert_eq!(generated, checked_in);
    }

    #[test]
    fn generated_runtime_api_contract_covers_current_v1_surface() {
        let contract = generate_runtime_api_contract();
        assert_eq!(contract.endpoints.len(), 37);
        assert!(contract
            .endpoints
            .iter()
            .any(|endpoint| endpoint.path == "/v1/jobs/run" && endpoint.method == "POST"));
        assert!(contract
            .endpoints
            .iter()
            .any(|endpoint| endpoint.path == "/a2a/hello" && endpoint.method == "POST"));
        assert!(contract
            .endpoints
            .iter()
            .any(|endpoint| endpoint.path == "/v1/workers/:worker_id/ack"
                && endpoint.method == "POST"));
        assert!(contract.schemas.contains_key("ApiEnvelope_RunJobResponse"));
    }
}
