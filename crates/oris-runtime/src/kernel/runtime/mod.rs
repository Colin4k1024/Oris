//! Phase 1 runtime skeleton modules for Oris OS.
//!
//! These modules define compile-safe interfaces for scheduler/lease/repository
//! without changing existing kernel behavior.

#[cfg(feature = "execution-server")]
pub mod api_errors;
#[cfg(feature = "execution-server")]
pub mod api_handlers;
#[cfg(all(feature = "execution-server", feature = "sqlite-persistence"))]
pub mod api_idempotency;
#[cfg(feature = "execution-server")]
pub mod api_models;
#[cfg(feature = "sqlite-persistence")]
pub mod backend_config;
pub mod lease;
pub mod models;
#[cfg(feature = "kernel-postgres")]
pub mod postgres_runtime_repository;
pub mod repository;
pub mod scheduler;
#[cfg(feature = "sqlite-persistence")]
pub mod sqlite_runtime_repository;

#[cfg(feature = "execution-server")]
pub use api_errors::ApiError;
#[cfg(feature = "execution-server")]
pub use api_handlers::{build_router, ApiRole, ExecutionApiState};
#[cfg(all(feature = "execution-server", feature = "sqlite-persistence"))]
pub use api_idempotency::{IdempotencyRecord, SqliteIdempotencyStore};
#[cfg(feature = "execution-server")]
pub use api_models::{
    ApiEnvelope, ApiMeta, AuditLogItem, AuditLogListResponse, CancelJobRequest, CancelJobResponse,
    CheckpointInspectResponse, InterruptDetailResponse, InterruptListResponse, JobDetailResponse,
    JobHistoryItem, JobHistoryResponse, JobStateResponse, JobTimelineItem, JobTimelineResponse,
    ListAuditLogsQuery, ListJobsResponse, RejectInterruptRequest, ReplayJobRequest,
    ResumeInterruptRequest, ResumeJobRequest, RunJobRequest, RunJobResponse,
    TimelineExportResponse, WorkerAckRequest, WorkerAckResponse, WorkerExtendLeaseRequest,
    WorkerHeartbeatRequest, WorkerLeaseResponse, WorkerPollRequest, WorkerPollResponse,
    WorkerReportStepRequest,
};
#[cfg(feature = "sqlite-persistence")]
pub use backend_config::{RuntimeStorageBackend, RuntimeStorageConfig};
pub use lease::{LeaseConfig, LeaseManager, LeaseTickResult, RepositoryLeaseManager};
pub use models::{
    AttemptDispatchRecord, AttemptExecutionStatus, InterruptRecord, LeaseRecord, RunRecord,
    RunRuntimeStatus,
};
#[cfg(feature = "kernel-postgres")]
pub use postgres_runtime_repository::PostgresRuntimeRepository;
pub use repository::RuntimeRepository;
pub use scheduler::{SchedulerDecision, SkeletonScheduler};
#[cfg(feature = "sqlite-persistence")]
pub use sqlite_runtime_repository::SqliteRuntimeRepository;
