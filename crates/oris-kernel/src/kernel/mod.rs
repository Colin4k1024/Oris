//! Oris Kernel API (2.0).
//!
//! Minimal complete set of interfaces: Run (identity), Event Log (source of truth),
//! Replay, Action (single channel for tools/external world, governable).
//! Graph and Agent compile down to StepFn; tools implement ActionExecutor.

pub mod action;
pub mod determinism_guard;
pub mod evidence_bundle;
pub mod driver;
pub mod event;
pub mod event_store;
pub mod execution_log;
pub mod execution_step;
pub mod execution_suspension;
pub mod identity;
pub mod interrupt;
pub mod interrupt_resolver;
pub mod kernel_interrupt;
pub mod kernel_mode;
pub mod policy;
#[cfg(feature = "kernel-postgres")]
pub mod postgres_store;
pub mod reducer;
pub mod replay_cursor;
pub mod replay_resume;
pub mod replay_verifier;
pub mod runner;
pub mod runtime_effect;
pub mod snapshot;
#[cfg(feature = "sqlite-persistence")]
pub mod sqlite_store;
pub mod state;
pub mod step;
pub mod stubs;
pub mod timeline;
pub mod timeline_fork;

pub use action::{Action, ActionError, ActionErrorKind, ActionExecutor, ActionResult};
pub use evidence_bundle::{EvidenceBundle, EvidenceBundleBuilder, TestOutcome};
pub use determinism_guard::{
    compute_event_stream_hash, event_stream_hash, verify_event_stream_hash, DeterminismGuard,
};
pub use driver::{BlockedInfo, Kernel, RunStatus, Signal};
pub use event::{Event, EventStore, KernelError, SequencedEvent};
pub use event_store::{InMemoryEventStore, SharedEventStore};
pub use execution_log::{scan_execution_log, scan_execution_trace, ExecutionLog, KernelTraceEvent};
pub use execution_step::{ExecutionStep, ExecutionStepInput, StepResult};
pub use execution_suspension::{ExecutionSuspension, ExecutionSuspensionState, SuspensionError};
pub use identity::{RunId, Seq, StepId};
pub use interrupt::{Interrupt, InterruptError, InterruptId, InterruptKind, InterruptStore};
pub use interrupt_resolver::{
    InterruptResolver, InterruptResolverError, InterruptSource, ResolveResult,
};
pub use kernel_interrupt::{
    InMemoryKernelInterruptStore, InterruptCheckpoint, KernelInterrupt, KernelInterruptError,
    KernelInterruptId, KernelInterruptKind, KernelInterruptStatus, KernelInterruptStore,
};
pub use kernel_mode::KernelMode;
pub use policy::{
    AllowListPolicy, BudgetRules, Policy, PolicyCtx, RetryDecision, RetryWithBackoffPolicy,
};
#[cfg(feature = "kernel-postgres")]
pub use postgres_store::{PostgresEventStore, PostgresSnapshotStore};
pub use reducer::{Reducer, StateUpdatedOnlyReducer};
pub use replay_cursor::{ReplayCursor, ReplayStepIter};
pub use replay_resume::{ReplayResume, ResumeDecision, ResumeResult};
pub use replay_verifier::{ReplayVerifier, VerificationFailure, VerificationResult, VerifyConfig};
pub use runner::KernelRunner;
pub use runtime_effect::{EffectSink, NoopEffectSink, RuntimeEffect};
pub use snapshot::{InMemorySnapshotStore, Snapshot, SnapshotStore};
#[cfg(feature = "sqlite-persistence")]
pub use sqlite_store::{SqliteEventStore, SqliteSnapshotStore};
pub use state::KernelState;
pub use step::{InterruptInfo, Next, StepFn};
pub use stubs::{AllowAllPolicy, NoopActionExecutor, NoopStepFn};
pub use timeline::{run_timeline, RunStatusSummary, RunTimeline, TimelineEntry};
pub use timeline_fork::{ForkResult, TimelineFork, TimelineForker};
