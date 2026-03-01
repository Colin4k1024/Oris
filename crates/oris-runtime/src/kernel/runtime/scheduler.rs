//! Scheduler skeleton for Phase 1 runtime rollout.

use chrono::Utc;

use crate::kernel::event::KernelError;

use super::models::AttemptDispatchRecord;
use super::repository::RuntimeRepository;

const DISPATCH_SCAN_LIMIT: usize = 16;

/// Scheduler dispatch decision.
#[derive(Clone, Debug)]
pub enum SchedulerDecision {
    Dispatched {
        attempt_id: String,
        worker_id: String,
    },
    Noop,
}

/// Compile-safe scheduler skeleton for queue -> lease dispatch.
pub struct SkeletonScheduler<R: RuntimeRepository> {
    repository: R,
}

impl<R: RuntimeRepository> SkeletonScheduler<R> {
    pub fn new(repository: R) -> Self {
        Self { repository }
    }

    /// Attempt to dispatch one eligible attempt to `worker_id`.
    pub fn dispatch_one(&self, worker_id: &str) -> Result<SchedulerDecision, KernelError> {
        let now = Utc::now();
        let candidates: Vec<AttemptDispatchRecord> = self
            .repository
            .list_dispatchable_attempts(now, DISPATCH_SCAN_LIMIT)?;
        let lease_expires_at = now + chrono::Duration::seconds(30);

        for candidate in candidates {
            if let Err(e) =
                self.repository
                    .upsert_lease(&candidate.attempt_id, worker_id, lease_expires_at)
            {
                let msg = e.to_string();
                if msg.contains("active lease already exists") || msg.contains("not dispatchable") {
                    continue;
                }
                return Err(e);
            }

            return Ok(SchedulerDecision::Dispatched {
                attempt_id: candidate.attempt_id,
                worker_id: worker_id.to_string(),
            });
        }

        Ok(SchedulerDecision::Noop)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::sync::{Arc, Mutex};

    use chrono::{DateTime, Utc};

    use super::*;
    use crate::kernel::identity::{RunId, Seq};

    use super::super::models::{AttemptExecutionStatus, LeaseRecord};

    #[derive(Clone)]
    struct FakeRepository {
        attempts: Vec<AttemptDispatchRecord>,
        conflict_attempts: Arc<Mutex<HashSet<String>>>,
        claimed_attempts: Arc<Mutex<Vec<String>>>,
    }

    impl FakeRepository {
        fn new(attempts: Vec<AttemptDispatchRecord>, conflict_attempts: &[&str]) -> Self {
            Self {
                attempts,
                conflict_attempts: Arc::new(Mutex::new(
                    conflict_attempts.iter().map(|s| (*s).to_string()).collect(),
                )),
                claimed_attempts: Arc::new(Mutex::new(Vec::new())),
            }
        }
    }

    impl RuntimeRepository for FakeRepository {
        fn list_dispatchable_attempts(
            &self,
            _now: DateTime<Utc>,
            _limit: usize,
        ) -> Result<Vec<AttemptDispatchRecord>, KernelError> {
            Ok(self.attempts.clone())
        }

        fn upsert_lease(
            &self,
            attempt_id: &str,
            worker_id: &str,
            lease_expires_at: DateTime<Utc>,
        ) -> Result<LeaseRecord, KernelError> {
            if self
                .conflict_attempts
                .lock()
                .expect("conflict lock")
                .contains(attempt_id)
            {
                return Err(KernelError::Driver(format!(
                    "active lease already exists for attempt: {}",
                    attempt_id
                )));
            }
            self.claimed_attempts
                .lock()
                .expect("claimed lock")
                .push(attempt_id.to_string());
            Ok(LeaseRecord {
                lease_id: format!("lease-{}", attempt_id),
                attempt_id: attempt_id.to_string(),
                worker_id: worker_id.to_string(),
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
            _stale_before: DateTime<Utc>,
        ) -> Result<u64, KernelError> {
            Ok(0)
        }

        fn latest_seq_for_run(&self, _run_id: &RunId) -> Result<Seq, KernelError> {
            Ok(0)
        }
    }

    fn attempt(id: &str, attempt_no: u32) -> AttemptDispatchRecord {
        AttemptDispatchRecord {
            attempt_id: id.to_string(),
            run_id: "run-scheduler-test".to_string(),
            attempt_no,
            status: AttemptExecutionStatus::Queued,
            retry_at: None,
        }
    }

    #[test]
    fn dispatch_one_skips_conflicted_candidate_and_preserves_order() {
        let repo = FakeRepository::new(
            vec![attempt("attempt-a", 1), attempt("attempt-b", 2)],
            &["attempt-a"],
        );
        let scheduler = SkeletonScheduler::new(repo.clone());

        let decision = scheduler
            .dispatch_one("worker-scheduler")
            .expect("dispatch should succeed");

        match decision {
            SchedulerDecision::Dispatched {
                attempt_id,
                worker_id,
            } => {
                assert_eq!(attempt_id, "attempt-b");
                assert_eq!(worker_id, "worker-scheduler");
            }
            SchedulerDecision::Noop => panic!("expected a dispatch"),
        }

        let claimed = repo.claimed_attempts.lock().expect("claimed lock");
        assert_eq!(claimed.as_slice(), ["attempt-b"]);
    }

    #[test]
    fn dispatch_one_returns_noop_when_all_candidates_conflict() {
        let repo = FakeRepository::new(
            vec![attempt("attempt-a", 1), attempt("attempt-b", 2)],
            &["attempt-a", "attempt-b"],
        );
        let scheduler = SkeletonScheduler::new(repo);

        let decision = scheduler
            .dispatch_one("worker-scheduler")
            .expect("conflicts should not surface as hard errors");

        assert!(matches!(decision, SchedulerDecision::Noop));
    }
}
