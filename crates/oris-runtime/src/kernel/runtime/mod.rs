//! Phase 1 runtime skeleton modules for Oris OS.
//!
//! These modules define compile-safe interfaces for scheduler/lease/repository
//! without changing existing kernel behavior.

pub mod lease;
pub mod models;
pub mod repository;
pub mod scheduler;

pub use lease::{LeaseConfig, LeaseManager, LeaseTickResult, RepositoryLeaseManager};
pub use models::{
    AttemptDispatchRecord, AttemptExecutionStatus, InterruptRecord, LeaseRecord, RunRecord,
    RunRuntimeStatus,
};
pub use repository::RuntimeRepository;
pub use scheduler::{SchedulerDecision, SkeletonScheduler};
