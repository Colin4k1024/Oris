//! Scheduler skeleton for Phase 1 runtime rollout.

use chrono::Utc;

use crate::kernel::event::KernelError;

use super::models::AttemptDispatchRecord;
use super::repository::RuntimeRepository;

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
        let mut candidates: Vec<AttemptDispatchRecord> =
            self.repository.list_dispatchable_attempts(now, 1)?;

        let Some(candidate) = candidates.pop() else {
            return Ok(SchedulerDecision::Noop);
        };

        let lease_expires_at = now + chrono::Duration::seconds(30);
        self.repository
            .upsert_lease(&candidate.attempt_id, worker_id, lease_expires_at)?;

        // TODO(phase1.1): update attempt status in same transaction as lease write
        // and enforce queue ordering/fairness/backpressure policy.
        Ok(SchedulerDecision::Dispatched {
            attempt_id: candidate.attempt_id,
            worker_id: worker_id.to_string(),
        })
    }
}
