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
        let stale_before = now - self.config.heartbeat_grace;
        let timed_out = self.repository.transition_timed_out_attempts(now)?;
        let expired = self.repository.expire_leases_and_requeue(stale_before)?;
        Ok(LeaseTickResult {
            timed_out,
            expired_requeued: expired,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::kernel::identity::{RunId, Seq};

    use super::super::models::{AttemptDispatchRecord, LeaseRecord};

    #[derive(Clone)]
    struct FakeRepository {
        timed_out: u64,
        expired: u64,
        seen_cutoff: Arc<Mutex<Option<DateTime<Utc>>>>,
    }

    impl FakeRepository {
        fn new(timed_out: u64, expired: u64) -> Self {
            Self {
                timed_out,
                expired,
                seen_cutoff: Arc::new(Mutex::new(None)),
            }
        }
    }

    impl RuntimeRepository for FakeRepository {
        fn list_dispatchable_attempts(
            &self,
            _now: DateTime<Utc>,
            _limit: usize,
        ) -> Result<Vec<AttemptDispatchRecord>, KernelError> {
            Ok(Vec::new())
        }

        fn upsert_lease(
            &self,
            _attempt_id: &str,
            _worker_id: &str,
            lease_expires_at: DateTime<Utc>,
        ) -> Result<LeaseRecord, KernelError> {
            Ok(LeaseRecord {
                lease_id: "lease-test".to_string(),
                attempt_id: "attempt-test".to_string(),
                worker_id: "worker-test".to_string(),
                lease_expires_at,
                heartbeat_at: Utc::now(),
                version: 1,
            })
        }

        fn heartbeat_lease(
            &self,
            _lease_id: &str,
            _heartbeat_at: DateTime<Utc>,
            _lease_expires_at: DateTime<Utc>,
        ) -> Result<(), KernelError> {
            Ok(())
        }

        fn expire_leases_and_requeue(
            &self,
            stale_before: DateTime<Utc>,
        ) -> Result<u64, KernelError> {
            *self.seen_cutoff.lock().expect("cutoff lock") = Some(stale_before);
            Ok(self.expired)
        }

        fn transition_timed_out_attempts(&self, _now: DateTime<Utc>) -> Result<u64, KernelError> {
            Ok(self.timed_out)
        }

        fn latest_seq_for_run(&self, _run_id: &RunId) -> Result<Seq, KernelError> {
            Ok(0)
        }
    }

    #[test]
    fn tick_applies_heartbeat_grace_before_requeueing() {
        let repo = FakeRepository::new(2, 3);
        let config = LeaseConfig {
            lease_ttl: Duration::seconds(30),
            heartbeat_grace: Duration::seconds(7),
        };
        let manager = RepositoryLeaseManager::new(repo.clone(), config);
        let now = Utc::now();

        let result = manager.tick(now).expect("tick succeeds");

        assert_eq!(result.timed_out, 2);
        assert_eq!(result.expired_requeued, 3);
        let seen_cutoff = repo
            .seen_cutoff
            .lock()
            .expect("cutoff lock")
            .expect("cutoff recorded");
        assert_eq!(seen_cutoff, now - Duration::seconds(7));
    }
}
