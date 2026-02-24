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
pub mod reducer;
pub mod runner;
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
pub use reducer::{Reducer, StateUpdatedOnlyReducer};
pub use runner::KernelRunner;
pub use snapshot::{InMemorySnapshotStore, Snapshot, SnapshotStore};
pub use state::KernelState;
pub use step::{InterruptInfo, Next, StepFn};
pub use stubs::{AllowAllPolicy, NoopActionExecutor, NoopStepFn};
pub use timeline::{run_timeline, RunStatusSummary, RunTimeline, TimelineEntry};
