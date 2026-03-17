//! Scheduler skeleton for Phase 1 runtime rollout.

use chrono::Utc;

use oris_kernel::event::KernelError;

use super::models::AttemptDispatchRecord;
use super::observability::RejectionReason;
use super::repository::RuntimeRepository;

const DISPATCH_SCAN_LIMIT: usize = 16;

/// Context for context-aware dispatch (tenant, priority, plugin/worker capabilities).
/// Used to route or filter work; concrete routing logic can be extended later.
#[derive(Clone, Debug, Default)]
pub struct DispatchContext {
    pub tenant_id: Option<String>,
    pub priority: Option<u32>,
    /// Plugin type names required for this dispatch (e.g. node kinds).
    pub plugin_requirements: Option<Vec<String>>,
    /// Worker capability tags the scheduler may match against.
    pub worker_capabilities: Option<Vec<String>>,
    /// Maximum queue depth before backpressure is applied.
    /// When the number of dispatchable candidates meets or exceeds this limit,
    /// `dispatch_one_with_context` returns `SchedulerDecision::Backpressure`
    /// instead of acquiring a new lease.
    pub max_queue_depth: Option<usize>,
}

impl DispatchContext {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_tenant(mut self, tenant_id: impl Into<String>) -> Self {
        self.tenant_id = Some(tenant_id.into());
        self
    }

    pub fn with_priority(mut self, priority: u32) -> Self {
        self.priority = Some(priority);
        self
    }

    pub fn with_max_queue_depth(mut self, limit: usize) -> Self {
        self.max_queue_depth = Some(limit);
        self
    }
}

/// Scheduler dispatch decision.
#[derive(Clone, Debug)]
pub enum SchedulerDecision {
    Dispatched {
        attempt_id: String,
        worker_id: String,
    },
    /// Backpressure applied: the queue depth has exceeded the configured limit.
    Backpressure {
        reason: RejectionReason,
        queue_depth: usize,
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
        self.dispatch_one_with_context(worker_id, None)
    }

    /// Dispatch one attempt to `worker_id` with optional context for tenant/priority/capability routing.
    /// Context is passed through for future filtering or sorting; current implementation
    /// uses the same candidate list as `dispatch_one`.
    ///
    /// If `context.max_queue_depth` is set and the number of dispatchable candidates is
    /// greater than or equal to that limit, returns `SchedulerDecision::Backpressure`
    /// without acquiring any lease.
    pub fn dispatch_one_with_context(
        &self,
        worker_id: &str,
        context: Option<&DispatchContext>,
    ) -> Result<SchedulerDecision, KernelError> {
        let now = Utc::now();
        let candidates: Vec<AttemptDispatchRecord> = self
            .repository
            .list_dispatchable_attempts(now, DISPATCH_SCAN_LIMIT)?;

        // Backpressure gate: if queue depth meets or exceeds the limit, reject dispatch.
        if let Some(limit) = context.and_then(|c| c.max_queue_depth) {
            if candidates.len() >= limit {
                return Ok(SchedulerDecision::Backpressure {
                    reason: RejectionReason::capacity_limit(format!(
                        "queue depth {} >= limit {}",
                        candidates.len(),
                        limit
                    )),
                    queue_depth: candidates.len(),
                });
            }
        }

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
    use oris_kernel::identity::{RunId, Seq};

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

        fn upsert_bounty(&self, _: &super::super::models::BountyRecord) -> Result<(), KernelError> {
            Ok(())
        }
        fn get_bounty(
            &self,
            _: &str,
        ) -> Result<Option<super::super::models::BountyRecord>, KernelError> {
            Ok(None)
        }
        fn list_bounties(
            &self,
            _: Option<&str>,
            _: usize,
        ) -> Result<Vec<super::super::models::BountyRecord>, KernelError> {
            Ok(vec![])
        }
        fn accept_bounty(&self, _: &str, _: &str) -> Result<(), KernelError> {
            Ok(())
        }
        fn close_bounty(&self, _: &str) -> Result<(), KernelError> {
            Ok(())
        }
        fn upsert_swarm_decomposition(
            &self,
            _: &super::super::models::SwarmTaskRecord,
        ) -> Result<(), KernelError> {
            Ok(())
        }
        fn get_swarm_decomposition(
            &self,
            _: &str,
        ) -> Result<Option<super::super::models::SwarmTaskRecord>, KernelError> {
            Ok(None)
        }
        fn register_worker(
            &self,
            _: &super::super::models::WorkerRecord,
        ) -> Result<(), KernelError> {
            Ok(())
        }
        fn get_worker(
            &self,
            _: &str,
        ) -> Result<Option<super::super::models::WorkerRecord>, KernelError> {
            Ok(None)
        }
        fn list_workers(
            &self,
            _: Option<&str>,
            _: Option<&str>,
            _: usize,
        ) -> Result<Vec<super::super::models::WorkerRecord>, KernelError> {
            Ok(vec![])
        }
        fn heartbeat_worker(&self, _: &str, _: i64) -> Result<(), KernelError> {
            Ok(())
        }
        fn create_recipe(&self, _: &super::super::models::RecipeRecord) -> Result<(), KernelError> {
            Ok(())
        }
        fn get_recipe(
            &self,
            _: &str,
        ) -> Result<Option<super::super::models::RecipeRecord>, KernelError> {
            Ok(None)
        }
        fn fork_recipe(
            &self,
            _: &str,
            _: &str,
            _: &str,
        ) -> Result<Option<super::super::models::RecipeRecord>, KernelError> {
            Ok(None)
        }
        fn list_recipes(
            &self,
            _: Option<&str>,
            _: usize,
        ) -> Result<Vec<super::super::models::RecipeRecord>, KernelError> {
            Ok(vec![])
        }
        fn express_organism(
            &self,
            _: &super::super::models::OrganismRecord,
        ) -> Result<(), KernelError> {
            Ok(())
        }
        fn get_organism(
            &self,
            _: &str,
        ) -> Result<Option<super::super::models::OrganismRecord>, KernelError> {
            Ok(None)
        }
        fn update_organism(&self, _: &str, _: i32, _: &str) -> Result<(), KernelError> {
            Ok(())
        }
        fn create_session(
            &self,
            _: &super::super::models::SessionRecord,
        ) -> Result<(), KernelError> {
            Ok(())
        }
        fn get_session(
            &self,
            _: &str,
        ) -> Result<Option<super::super::models::SessionRecord>, KernelError> {
            Ok(None)
        }
        fn add_session_message(
            &self,
            _: &super::super::models::SessionMessageRecord,
        ) -> Result<(), KernelError> {
            Ok(())
        }
        fn get_session_history(
            &self,
            _: &str,
            _: usize,
        ) -> Result<Vec<super::super::models::SessionMessageRecord>, KernelError> {
            Ok(vec![])
        }
        fn open_dispute(&self, _: &super::super::models::DisputeRecord) -> Result<(), KernelError> {
            Ok(())
        }
        fn get_dispute(
            &self,
            _: &str,
        ) -> Result<Option<super::super::models::DisputeRecord>, KernelError> {
            Ok(None)
        }
        fn get_disputes_for_bounty(
            &self,
            _: &str,
        ) -> Result<Vec<super::super::models::DisputeRecord>, KernelError> {
            Ok(vec![])
        }
        fn resolve_dispute(&self, _: &str, _: &str, _: &str) -> Result<(), KernelError> {
            Ok(())
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
            SchedulerDecision::Noop | SchedulerDecision::Backpressure { .. } => {
                panic!("expected a dispatch")
            }
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

    #[test]
    fn dispatch_one_with_context_none_same_as_dispatch_one() {
        let repo = FakeRepository::new(vec![attempt("attempt-a", 1)], &[]);
        let scheduler = SkeletonScheduler::new(repo.clone());

        let with_ctx = scheduler
            .dispatch_one_with_context("worker-1", None)
            .expect("dispatch should succeed");
        let without = scheduler
            .dispatch_one("worker-1")
            .expect("dispatch should succeed");

        match (&with_ctx, &without) {
            (
                SchedulerDecision::Dispatched { attempt_id: a1, .. },
                SchedulerDecision::Dispatched { attempt_id: a2, .. },
            ) => assert_eq!(a1, a2),
            _ => panic!("expected both dispatched"),
        }
    }

    #[test]
    fn dispatch_context_builder() {
        let ctx = DispatchContext::new()
            .with_tenant("tenant-1")
            .with_priority(5);
        assert_eq!(ctx.tenant_id.as_deref(), Some("tenant-1"));
        assert_eq!(ctx.priority, Some(5));
    }

    #[test]
    fn dispatch_one_with_context_applies_backpressure_when_queue_exceeds_limit() {
        // 3 queued attempts, limit = 2  → backpressure should fire
        let repo = FakeRepository::new(
            vec![
                attempt("attempt-a", 1),
                attempt("attempt-b", 2),
                attempt("attempt-c", 3),
            ],
            &[],
        );
        let scheduler = SkeletonScheduler::new(repo);
        let ctx = DispatchContext::new().with_max_queue_depth(2);

        let decision = scheduler
            .dispatch_one_with_context("worker-1", Some(&ctx))
            .expect("should not error on backpressure");

        match decision {
            SchedulerDecision::Backpressure { queue_depth, .. } => {
                assert_eq!(queue_depth, 3);
            }
            other => panic!("expected Backpressure, got {:?}", other),
        }
    }

    #[test]
    fn dispatch_one_with_context_dispatches_when_queue_below_limit() {
        // 1 queued attempt, limit = 5 → should dispatch normally
        let repo = FakeRepository::new(vec![attempt("attempt-a", 1)], &[]);
        let scheduler = SkeletonScheduler::new(repo);
        let ctx = DispatchContext::new().with_max_queue_depth(5);

        let decision = scheduler
            .dispatch_one_with_context("worker-1", Some(&ctx))
            .expect("dispatch should succeed");

        assert!(
            matches!(decision, SchedulerDecision::Dispatched { .. }),
            "expected Dispatched, got {:?}",
            decision
        );
    }
}
