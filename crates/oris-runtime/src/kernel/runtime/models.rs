//! Runtime domain models for Phase 1 skeleton.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::kernel::identity::{RunId, Seq};

/// Runtime-level status of a run for control-plane orchestration.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum RunRuntimeStatus {
    Queued,
    Leased,
    Running,
    BlockedInterrupt,
    RetryBackoff,
    Completed,
    Failed,
    Cancelled,
}

/// Runtime-level status of an execution attempt.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum AttemptExecutionStatus {
    Queued,
    Leased,
    Running,
    RetryBackoff,
    Completed,
    Failed,
    Cancelled,
}

/// Run metadata record for scheduler/control-plane usage.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RunRecord {
    pub run_id: RunId,
    pub workflow_name: String,
    pub status: RunRuntimeStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Candidate attempt returned by repository for scheduler dispatch.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AttemptDispatchRecord {
    pub attempt_id: String,
    pub run_id: RunId,
    pub attempt_no: u32,
    pub status: AttemptExecutionStatus,
    pub retry_at: Option<DateTime<Utc>>,
}

/// Lease metadata for worker ownership and failover.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LeaseRecord {
    pub lease_id: String,
    pub attempt_id: String,
    pub worker_id: String,
    pub lease_expires_at: DateTime<Utc>,
    pub heartbeat_at: DateTime<Utc>,
    pub version: u64,
}

/// Interrupt metadata record for operator resume flow.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InterruptRecord {
    pub interrupt_id: String,
    pub run_id: RunId,
    pub attempt_id: String,
    pub event_seq: Seq,
    pub is_pending: bool,
}
