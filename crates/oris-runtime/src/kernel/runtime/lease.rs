//! Lease management skeleton for Phase 1.

use chrono::{DateTime, Duration, Utc};

use crate::kernel::event::KernelError;

use super::repository::RuntimeRepository;

/// Lease behavior tuning knobs for scheduler/data-plane coordination.
#[derive(Clone, Debug)]
pub struct LeaseConfig {
    pub lease_ttl: Duration,
    pub heartbeat_grace: Duration,
}

impl Default for LeaseConfig {
    fn default() -> Self {
        Self {
            lease_ttl: Duration::seconds(30),
            heartbeat_grace: Duration::seconds(5),
        }
    }
}

/// Result of a periodic lease tick.
#[derive(Clone, Debug, Default)]
pub struct LeaseTickResult {
    pub timed_out: u64,
    pub expired_requeued: u64,
}

/// Lease manager abstraction.
pub trait LeaseManager: Send + Sync {
    fn tick(&self, now: DateTime<Utc>) -> Result<LeaseTickResult, KernelError>;
}

/// Skeleton lease manager using `RuntimeRepository`.
pub struct RepositoryLeaseManager<R: RuntimeRepository> {
    repository: R,
    config: LeaseConfig,
}

impl<R: RuntimeRepository> RepositoryLeaseManager<R> {
    pub fn new(repository: R, config: LeaseConfig) -> Self {
        Self { repository, config }
    }
}

impl<R: RuntimeRepository> LeaseManager for RepositoryLeaseManager<R> {
    fn tick(&self, now: DateTime<Utc>) -> Result<LeaseTickResult, KernelError> {
        // TODO(phase1.1): use `heartbeat_grace` and lease versions to implement
        // stricter expiry semantics and split-brain protections.
        let _next_expiry_scan_cutoff = now - self.config.heartbeat_grace;
        let timed_out = self.repository.transition_timed_out_attempts(now)?;
        let expired = self.repository.expire_leases_and_requeue(now)?;
        Ok(LeaseTickResult {
            timed_out,
            expired_requeued: expired,
        })
    }
}
