//! Storage façade for runtime scheduler/lease operations.

use chrono::{DateTime, Utc};

use crate::kernel::event::KernelError;
use crate::kernel::identity::{RunId, Seq};

use super::models::{AttemptDispatchRecord, LeaseRecord};

/// Runtime repository contract used by scheduler and lease manager.
///
/// TODO(phase1.1): Back this trait with real SQL implementations and
/// transactional ordering constraints in Postgres.
pub trait RuntimeRepository: Send + Sync {
    /// Return attempts eligible for dispatch at the current time.
    fn list_dispatchable_attempts(
        &self,
        now: DateTime<Utc>,
        limit: usize,
    ) -> Result<Vec<AttemptDispatchRecord>, KernelError>;

    /// Create or replace a lease for an attempt.
    fn upsert_lease(
        &self,
        attempt_id: &str,
        worker_id: &str,
        lease_expires_at: DateTime<Utc>,
    ) -> Result<LeaseRecord, KernelError>;

    /// Refresh heartbeat for an existing lease.
    fn heartbeat_lease(
        &self,
        lease_id: &str,
        heartbeat_at: DateTime<Utc>,
        lease_expires_at: DateTime<Utc>,
    ) -> Result<(), KernelError>;

    /// Expire stale leases and requeue affected attempts.
    fn expire_leases_and_requeue(&self, now: DateTime<Utc>) -> Result<u64, KernelError>;

    /// Returns latest persisted sequence for a run (used by replay wiring).
    fn latest_seq_for_run(&self, run_id: &RunId) -> Result<Seq, KernelError>;
}
