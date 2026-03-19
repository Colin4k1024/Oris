//! Execution runtime control-plane, scheduler, and durable runtime repositories.

#[cfg(feature = "execution-server")]
pub mod api_contract;
#[cfg(feature = "execution-server")]
pub mod api_errors;
#[cfg(all(feature = "execution-server", feature = "sqlite-persistence"))]
pub mod api_idempotency;
#[cfg(feature = "execution-server")]
pub mod api_models;
#[cfg(feature = "sqlite-persistence")]
pub mod backend_config;
pub mod circuit_breaker;
#[cfg(feature = "execution-server")]
pub mod graph_bridge;
pub mod lease;
pub mod models;
pub mod observability;
#[cfg(feature = "kernel-postgres")]
pub mod postgres_runtime_repository;
pub mod recovery;
pub mod repository;
pub mod scheduler;
#[cfg(feature = "sqlite-persistence")]
pub mod sqlite_runtime_repository;

#[cfg(feature = "execution-server")]
pub use api_contract::{
    canonical_runtime_api_contract_path, generate_runtime_api_contract,
    runtime_api_contract_pretty_json, write_runtime_api_contract, RUNTIME_API_CONTRACT_DOC_PATH,
};
#[cfg(feature = "execution-server")]
pub use api_errors::ApiError;
#[cfg(all(feature = "execution-server", feature = "sqlite-persistence"))]
pub use api_idempotency::{IdempotencyRecord, SqliteIdempotencyStore};
#[cfg(feature = "execution-server")]
pub use api_models::{
    ApiEnvelope, ApiMeta, AttemptRetryHistoryItem, AttemptRetryHistoryResponse, AuditLogItem,
    AuditLogListResponse, CancelJobRequest, CancelJobResponse, CheckpointInspectResponse,
    DeadLetterItem, DeadLetterListResponse, DeadLetterReplayResponse, InterruptDetailResponse,
    InterruptListResponse, JobDetailResponse, JobHistoryItem, JobHistoryResponse, JobStateResponse,
    JobTimelineItem, JobTimelineResponse, ListAuditLogsQuery, ListDeadLettersQuery,
    ListInterruptsQuery, ListJobsQuery, ListJobsResponse, RejectInterruptRequest, ReplayJobRequest,
    ResumeInterruptRequest, ResumeJobRequest, RetryPolicyRequest, RunJobRequest, RunJobResponse,
    TimelineExportResponse, TimeoutPolicyRequest, TraceContextResponse, WorkerAckRequest,
    WorkerAckResponse, WorkerExtendLeaseRequest, WorkerHeartbeatRequest, WorkerLeaseResponse,
    WorkerPollRequest, WorkerPollResponse, WorkerReportStepRequest,
};
#[cfg(feature = "sqlite-persistence")]
pub use backend_config::{RuntimeStorageBackend, RuntimeStorageConfig};
pub use circuit_breaker::{CircuitBreaker, CircuitState};
#[cfg(feature = "execution-server")]
pub use graph_bridge::{
    ExecutionCheckpointView, ExecutionGraphBridge, ExecutionGraphBridgeError,
    ExecutionGraphBridgeErrorKind, ExecutionInvokeView, ExecutionStateView,
};
pub use lease::{
    LeaseConfig, LeaseManager, LeaseTickResult, RepositoryLeaseManager, WorkerHealth,
    WorkerHealthTracker, WorkerLease,
};
pub use models::{
    AttemptDispatchRecord, AttemptExecutionStatus, InterruptRecord, LeaseRecord, LeaseTerminalState, RunRecord,
    RunRuntimeStatus,
};
pub use observability::{KernelObservability, RejectionReason};
#[cfg(feature = "kernel-postgres")]
pub use postgres_runtime_repository::{
    PostgresIdempotencyRecord, PostgresIdempotencyStore, PostgresRuntimeRepository,
};
pub use recovery::{CrashRecoveryPipeline, RecoveryContext, RecoveryStep};
pub use repository::RuntimeRepository;
pub use scheduler::{DispatchContext, SchedulerDecision, SkeletonScheduler};
#[cfg(feature = "sqlite-persistence")]
pub use sqlite_runtime_repository::SqliteRuntimeRepository;
