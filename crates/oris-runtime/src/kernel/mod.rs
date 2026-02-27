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
    build_router, ApiEnvelope, ApiError, ApiMeta, ApiRole, AuditLogItem, AuditLogListResponse,
    CancelJobRequest, CancelJobResponse, CheckpointInspectResponse, ExecutionApiState,
    InterruptDetailResponse, InterruptListResponse, JobDetailResponse, JobHistoryItem,
    JobHistoryResponse, JobStateResponse, JobTimelineItem, JobTimelineResponse, ListAuditLogsQuery,
    ListJobsResponse, RejectInterruptRequest, ReplayJobRequest, ResumeInterruptRequest,
    ResumeJobRequest, RunJobRequest, RunJobResponse, TimelineExportResponse, WorkerAckRequest,
    WorkerAckResponse, WorkerExtendLeaseRequest, WorkerHeartbeatRequest, WorkerLeaseResponse,
    WorkerPollRequest, WorkerPollResponse, WorkerReportStepRequest,
};
pub use runtime::{
    AttemptDispatchRecord, AttemptExecutionStatus, InterruptRecord, LeaseConfig, LeaseManager,
    LeaseRecord, LeaseTickResult, RepositoryLeaseManager, RunRecord, RunRuntimeStatus,
    RuntimeRepository, SchedulerDecision, SkeletonScheduler,
};
#[cfg(all(feature = "execution-server", feature = "sqlite-persistence"))]
pub use runtime::{IdempotencyRecord, SqliteIdempotencyStore, SqliteRuntimeRepository};
pub use snapshot::{InMemorySnapshotStore, Snapshot, SnapshotStore};
pub use state::KernelState;
pub use step::{InterruptInfo, Next, StepFn};
pub use stubs::{AllowAllPolicy, NoopActionExecutor, NoopStepFn};
pub use timeline::{run_timeline, RunStatusSummary, RunTimeline, TimelineEntry};
