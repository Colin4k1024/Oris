//! Graph-aware HTTP execution server facade for Oris.
//!
//! This crate is the stable package entry point for the execution-server surface.
//! Today it forwards to `oris-runtime`, while the graph engine still lives there.
//! Once the graph API is extracted further, the implementation can move here
//! without breaking downstream import paths.

#![cfg_attr(docsrs, feature(doc_cfg))]

#[cfg(feature = "execution-server")]
pub use oris_runtime::execution_server::*;

#[cfg(feature = "execution-server")]
pub use oris_runtime::execution_runtime::{
    canonical_runtime_api_contract_path, generate_runtime_api_contract,
    runtime_api_contract_pretty_json, write_runtime_api_contract, ApiEnvelope, ApiError, ApiMeta,
    AttemptRetryHistoryItem, AttemptRetryHistoryResponse, AuditLogItem, AuditLogListResponse,
    CancelJobRequest, CancelJobResponse, CheckpointInspectResponse, DeadLetterItem,
    DeadLetterListResponse, DeadLetterReplayResponse, ExecutionCheckpointView,
    ExecutionGraphBridge, ExecutionGraphBridgeError, ExecutionGraphBridgeErrorKind,
    ExecutionInvokeView, ExecutionStateView, InterruptDetailResponse, InterruptListResponse,
    JobDetailResponse, JobHistoryItem, JobHistoryResponse, JobStateResponse, JobTimelineItem,
    JobTimelineResponse, KernelObservability, ListAuditLogsQuery, ListDeadLettersQuery,
    ListInterruptsQuery, ListJobsQuery, ListJobsResponse, RejectInterruptRequest, ReplayJobRequest,
    ResumeInterruptRequest, ResumeJobRequest, RetryPolicyRequest, RunJobRequest, RunJobResponse,
    TimelineExportResponse, TimeoutPolicyRequest, TraceContextResponse, WorkerAckRequest,
    WorkerAckResponse, WorkerExtendLeaseRequest, WorkerHeartbeatRequest, WorkerLeaseResponse,
    WorkerPollRequest, WorkerPollResponse, WorkerReportStepRequest, RUNTIME_API_CONTRACT_DOC_PATH,
};

#[cfg(feature = "sqlite-persistence")]
pub use oris_runtime::execution_runtime::{
    RuntimeStorageBackend, RuntimeStorageConfig, SqliteIdempotencyStore, SqliteRuntimeRepository,
};

#[cfg(any(
    feature = "evolution-network",
    feature = "evolution-network-experimental"
))]
pub use oris_runtime::evolution_network::*;
