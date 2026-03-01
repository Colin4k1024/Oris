//! Oris Kernel API (2.0).
//!
//! Minimal complete set of interfaces: Run (identity), Event Log (source of truth),
//! Replay, Action (single channel for tools/external world, governable).
//! Graph and Agent compile down to StepFn; tools implement ActionExecutor.

pub mod action;
pub mod driver;
pub mod event;
pub mod event_store;
pub mod identity;
pub mod policy;
#[cfg(feature = "kernel-postgres")]
pub mod postgres_store;
pub mod reducer;
pub mod runner;
pub mod runtime;
pub mod snapshot;
pub mod state;
pub mod step;
pub mod stubs;
pub mod timeline;

pub use action::{Action, ActionError, ActionErrorKind, ActionExecutor, ActionResult};
pub use driver::{BlockedInfo, Kernel, RunStatus, Signal};
pub use event::{Event, EventStore, KernelError, SequencedEvent};
pub use event_store::{InMemoryEventStore, SharedEventStore};
pub use identity::{RunId, Seq, StepId};
pub use policy::{
    AllowListPolicy, BudgetRules, Policy, PolicyCtx, RetryDecision, RetryWithBackoffPolicy,
};
#[cfg(feature = "kernel-postgres")]
pub use postgres_store::{PostgresEventStore, PostgresSnapshotStore};
pub use reducer::{Reducer, StateUpdatedOnlyReducer};
pub use runner::KernelRunner;
#[cfg(feature = "kernel-postgres")]
pub use runtime::PostgresRuntimeRepository;
#[cfg(feature = "execution-server")]
pub use runtime::{
    build_router, canonical_runtime_api_contract_path, generate_runtime_api_contract,
    runtime_api_contract_pretty_json, write_runtime_api_contract, ApiEnvelope, ApiError, ApiMeta,
    ApiRole, AttemptRetryHistoryItem, AttemptRetryHistoryResponse, AuditLogItem,
    AuditLogListResponse, CancelJobRequest, CancelJobResponse, CheckpointInspectResponse,
    DeadLetterItem, DeadLetterListResponse, DeadLetterReplayResponse, ExecutionApiState,
    InterruptDetailResponse, InterruptListResponse, JobDetailResponse, JobHistoryItem,
    JobHistoryResponse, JobStateResponse, JobTimelineItem, JobTimelineResponse, ListAuditLogsQuery,
    ListDeadLettersQuery, ListInterruptsQuery, ListJobsQuery, ListJobsResponse,
    RejectInterruptRequest, ReplayJobRequest, ResumeInterruptRequest, ResumeJobRequest,
    RetryPolicyRequest, RunJobRequest, RunJobResponse, TimelineExportResponse,
    TimeoutPolicyRequest, TraceContextResponse, WorkerAckRequest, WorkerAckResponse,
    WorkerExtendLeaseRequest, WorkerHeartbeatRequest, WorkerLeaseResponse, WorkerPollRequest,
    WorkerPollResponse, WorkerReportStepRequest, RUNTIME_API_CONTRACT_DOC_PATH,
};
#[cfg(all(feature = "execution-server", feature = "sqlite-persistence"))]
pub use runtime::{
    canonical_runtime_benchmark_baseline_path, run_runtime_benchmark_suite,
    runtime_benchmark_suite_pretty_json, write_runtime_benchmark_suite,
    RuntimeBenchmarkEnvironment, RuntimeBenchmarkMetric, RuntimeBenchmarkSuiteReport,
    RUNTIME_BENCHMARK_BASELINE_DOC_PATH,
};
pub use runtime::{
    AttemptDispatchRecord, AttemptExecutionStatus, InterruptRecord, LeaseConfig, LeaseManager,
    LeaseRecord, LeaseTickResult, RepositoryLeaseManager, RunRecord, RunRuntimeStatus,
    RuntimeRepository, SchedulerDecision, SkeletonScheduler,
};
#[cfg(all(feature = "execution-server", feature = "sqlite-persistence"))]
pub use runtime::{IdempotencyRecord, SqliteIdempotencyStore, SqliteRuntimeRepository};
#[cfg(feature = "sqlite-persistence")]
pub use runtime::{RuntimeStorageBackend, RuntimeStorageConfig};
pub use snapshot::{InMemorySnapshotStore, Snapshot, SnapshotStore};
pub use state::KernelState;
pub use step::{InterruptInfo, Next, StepFn};
pub use stubs::{AllowAllPolicy, NoopActionExecutor, NoopStepFn};
pub use timeline::{run_timeline, RunStatusSummary, RunTimeline, TimelineEntry};
