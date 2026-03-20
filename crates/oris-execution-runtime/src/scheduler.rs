//! Scheduler skeleton for Phase 1 runtime rollout.
//!
//! K5-c: Context-Aware Scheduler Kernel with priority-aware dispatch and configurable fairness.
//! K5-d: Safe Backpressure Engine with per-tenant and per-worker throttle limits.

use std::collections::HashMap;
use std::sync::Arc;

use chrono::Utc;

use oris_kernel::event::KernelError;

use super::circuit_breaker::CircuitBreaker;
use super::models::AttemptDispatchRecord;
use super::observability::RejectionReason;
use super::repository::RuntimeRepository;

const DISPATCH_SCAN_LIMIT: usize = 16;

/// Fairness policy for the scheduler (K5-c).
#[derive(Clone, Debug, Default)]
pub enum FairnessPolicy {
    /// First-come-first-served (default)
    #[default]
    FCFS,
    /// Priority-based with configurable weight
    PriorityWeighted { default_weight: u32 },
    /// Round-robin across tenants
    RoundRobin,
}

/// Priority level for dispatch candidates (K5-c).
#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct ThreadPriority(pub u32);

impl ThreadPriority {
    pub const LOW: ThreadPriority = ThreadPriority(0);
    pub const NORMAL: ThreadPriority = ThreadPriority(100);
    pub const HIGH: ThreadPriority = ThreadPriority(200);
    pub const CRITICAL: ThreadPriority = ThreadPriority(300);
}

/// Resource budget for a dispatch candidate (K5-c).
#[derive(Clone, Debug, Default)]
pub struct ResourceBudget {
    /// Maximum CPU units (0-1000)
    pub cpu_units: u32,
    /// Maximum memory in MB
    pub memory_mb: u32,
    /// Maximum concurrent actions
    pub max_concurrent_actions: u32,
}

impl ResourceBudget {
    pub fn unbounded() -> Self {
        Self {
            cpu_units: 1000,
            memory_mb: u32::MAX,
            max_concurrent_actions: 10,
        }
    }
}

/// Throttle limits for backpressure (K5-d).
#[derive(Clone, Debug)]
pub struct ThrottleLimits {
    /// Maximum concurrent runs per tenant
    pub max_concurrent_runs_per_tenant: usize,
    /// Maximum concurrent leases per worker
    pub max_concurrent_leases_per_worker: usize,
    /// Maximum queue depth before global backpressure
    pub max_queue_depth: usize,
}

impl Default for ThrottleLimits {
    fn default() -> Self {
        Self {
            max_concurrent_runs_per_tenant: 100,
            max_concurrent_leases_per_worker: 10,
            max_queue_depth: 1000,
        }
    }
}

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
    /// Fairness policy for this dispatch (K5-c)
    pub fairness_policy: Option<FairnessPolicy>,
    /// Thread priority for this dispatch (K5-c)
    pub thread_priority: Option<ThreadPriority>,
    /// Resource budget for this dispatch (K5-c)
    pub resource_budget: Option<ResourceBudget>,
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

    pub fn with_fairness_policy(mut self, policy: FairnessPolicy) -> Self {
        self.fairness_policy = Some(policy);
        self
    }

    pub fn with_thread_priority(mut self, priority: ThreadPriority) -> Self {
        self.thread_priority = Some(priority);
        self
    }

    pub fn with_resource_budget(mut self, budget: ResourceBudget) -> Self {
        self.resource_budget = Some(budget);
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

/// Compile-safe scheduler skeleton for queue -> lease dispatch with context-awareness (K5-c, K5-d).
pub struct SkeletonScheduler<R: RuntimeRepository> {
    repository: R,
    /// Optional circuit breaker applied globally to all dispatch calls.
    circuit_breaker: Option<Arc<CircuitBreaker>>,
    /// Fairness policy for dispatch (K5-c)
    fairness_policy: FairnessPolicy,
    /// Throttle limits for backpressure (K5-d)
    throttle_limits: ThrottleLimits,
    /// Current per-tenant run counts (for backpressure)
    tenant_run_counts: std::sync::Mutex<HashMap<String, usize>>,
    /// Current per-worker lease counts (for backpressure)
    worker_lease_counts: std::sync::Mutex<HashMap<String, usize>>,
}

impl<R: RuntimeRepository> SkeletonScheduler<R> {
    pub fn new(repository: R) -> Self {
        Self {
            repository,
            circuit_breaker: None,
            fairness_policy: FairnessPolicy::default(),
            throttle_limits: ThrottleLimits::default(),
            tenant_run_counts: std::sync::Mutex::new(HashMap::new()),
            worker_lease_counts: std::sync::Mutex::new(HashMap::new()),
        }
    }

    /// Attach a shared circuit breaker to this scheduler.
    ///
    /// When the breaker is `Open`, all dispatch calls return
    /// `SchedulerDecision::Backpressure` until the probe window elapses.
    pub fn with_circuit_breaker(mut self, breaker: Arc<CircuitBreaker>) -> Self {
        self.circuit_breaker = Some(breaker);
        self
    }

    /// Set fairness policy for the scheduler (K5-c).
    pub fn with_fairness_policy(mut self, policy: FairnessPolicy) -> Self {
        self.fairness_policy = policy;
        self
    }

    /// Set throttle limits for backpressure (K5-d).
    pub fn with_throttle_limits(mut self, limits: ThrottleLimits) -> Self {
        self.throttle_limits = limits;
        self
    }

    /// Attempt to dispatch one eligible attempt to `worker_id`.
    pub fn dispatch_one(&self, worker_id: &str) -> Result<SchedulerDecision, KernelError> {
        self.dispatch_one_with_context(worker_id, None)
    }

    /// Sort candidates based on fairness policy and priority (K5-c).
    fn sort_candidates(
        &self,
        candidates: &mut [AttemptDispatchRecord],
        context: Option<&DispatchContext>,
    ) {
        let policy = context
            .and_then(|c| c.fairness_policy.as_ref())
            .unwrap_or(&self.fairness_policy);

        match policy {
            FairnessPolicy::FCFS => {
                // Already in order from repository
            }
            FairnessPolicy::PriorityWeighted { .. } => {
                // Sort by priority (higher first)
                // Note: AttemptDispatchRecord doesn't have priority, so we use attempt_no as proxy
                candidates.sort_by(|a, b| b.attempt_no.cmp(&a.attempt_no));
            }
            FairnessPolicy::RoundRobin => {
                // For round-robin, we could track last dispatched tenant
                // For now, just use FCFS
            }
        }
    }

    /// Check per-tenant backpressure (K5-d).
    fn check_tenant_backpressure(
        &self,
        tenant_id: Option<&str>,
    ) -> Option<(RejectionReason, usize)> {
        if let Some(tenant) = tenant_id {
            let counts = self.tenant_run_counts.lock().unwrap();
            if let Some(&count) = counts.get(tenant) {
                if count >= self.throttle_limits.max_concurrent_runs_per_tenant {
                    return Some((
                        RejectionReason::tenant_limit(format!(
                            "tenant {} at {} runs, limit {}",
                            tenant, count, self.throttle_limits.max_concurrent_runs_per_tenant
                        )),
                        count,
                    ));
                }
            }
        }
        None
    }

    /// Check per-worker backpressure (K5-d).
    fn check_worker_backpressure(&self, worker_id: &str) -> Option<(RejectionReason, usize)> {
        let counts = self.worker_lease_counts.lock().unwrap();
        if let Some(&count) = counts.get(worker_id) {
            if count >= self.throttle_limits.max_concurrent_leases_per_worker {
                return Some((
                    RejectionReason::capacity_limit(format!(
                        "worker {} at {} leases, limit {}",
                        worker_id, count, self.throttle_limits.max_concurrent_leases_per_worker
                    )),
                    count,
                ));
            }
        }
        None
    }

    /// Increment tenant run count after successful dispatch (K5-d).
    fn increment_tenant_count(&self, tenant_id: Option<&str>) {
        if let Some(tenant) = tenant_id {
            let mut counts = self.tenant_run_counts.lock().unwrap();
            *counts.entry(tenant.to_string()).or_insert(0) += 1;
        }
    }

    /// Increment worker lease count after successful dispatch (K5-d).
    fn increment_worker_count(&self, worker_id: &str) {
        let mut counts = self.worker_lease_counts.lock().unwrap();
        *counts.entry(worker_id.to_string()).or_insert(0) += 1;
    }

    /// Decrement worker lease count when lease is released (K5-d).
    pub fn decrement_worker_count(&self, worker_id: &str) {
        let mut counts = self.worker_lease_counts.lock().unwrap();
        if let Some(count) = counts.get_mut(worker_id) {
            if *count > 0 {
                *count -= 1;
            }
        }
    }

    /// Get current scheduler metrics for observability (K5-d).
    pub fn get_metrics(&self) -> SchedulerMetrics {
        let tenant_counts = self.tenant_run_counts.lock().unwrap();
        let worker_counts = self.worker_lease_counts.lock().unwrap();

        SchedulerMetrics {
            tenant_run_counts: tenant_counts.clone(),
            worker_lease_counts: worker_counts.clone(),
            throttle_limits: self.throttle_limits.clone(),
        }
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

        // Circuit breaker gate: if the breaker is Open, reject dispatch.
        if let Some(cb) = &self.circuit_breaker {
            if cb.is_open() {
                return Ok(SchedulerDecision::Backpressure {
                    reason: RejectionReason::capacity_limit("circuit breaker open"),
                    queue_depth: 0,
                });
            }
        }

        let candidates: Vec<AttemptDispatchRecord> = self
            .repository
            .list_dispatchable_attempts(now, DISPATCH_SCAN_LIMIT)?;

        // Sort candidates based on fairness policy (K5-c)
        let mut sorted_candidates = candidates.clone();
        self.sort_candidates(&mut sorted_candidates, context);

        // Backpressure gate: if queue depth meets or exceeds the limit, reject dispatch.
        if let Some(limit) = context.and_then(|c| c.max_queue_depth) {
            if sorted_candidates.len() >= limit {
                return Ok(SchedulerDecision::Backpressure {
                    reason: RejectionReason::capacity_limit(format!(
                        "queue depth {} >= limit {}",
                        sorted_candidates.len(),
                        limit
                    )),
                    queue_depth: sorted_candidates.len(),
                });
            }
        }

        // Per-tenant backpressure check (K5-d)
        if let Some(tenant_id) = context.and_then(|c| c.tenant_id.as_deref()) {
            if let Some((reason, count)) = self.check_tenant_backpressure(Some(tenant_id)) {
                return Ok(SchedulerDecision::Backpressure {
                    reason,
                    queue_depth: count,
                });
            }
        }

        // Per-worker backpressure check (K5-d)
        if let Some((reason, count)) = self.check_worker_backpressure(worker_id) {
            return Ok(SchedulerDecision::Backpressure {
                reason,
                queue_depth: count,
            });
        }

        let lease_expires_at = now + chrono::Duration::seconds(30);

        for candidate in sorted_candidates {
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

            // Update counts after successful dispatch (K5-d)
            self.increment_tenant_count(context.and_then(|c| c.tenant_id.as_deref()));
            self.increment_worker_count(worker_id);

            return Ok(SchedulerDecision::Dispatched {
                attempt_id: candidate.attempt_id,
                worker_id: worker_id.to_string(),
            });
        }

        Ok(SchedulerDecision::Noop)
    }
}

/// Scheduler metrics for observability (K5-d).
#[derive(Clone, Debug)]
pub struct SchedulerMetrics {
    pub tenant_run_counts: HashMap<String, usize>,
    pub worker_lease_counts: HashMap<String, usize>,
    pub throttle_limits: ThrottleLimits,
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
                terminal_state: None,
                terminal_at: None,
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

    #[test]
    fn open_circuit_breaker_returns_backpressure() {
        use crate::circuit_breaker::CircuitBreaker;
        use std::sync::Arc;

        let repo = FakeRepository::new(vec![attempt("attempt-a", 1)], &[]);
        let breaker = Arc::new(CircuitBreaker::new(30));
        breaker.trip();

        let scheduler = SkeletonScheduler::new(repo).with_circuit_breaker(breaker);

        let decision = scheduler
            .dispatch_one("worker-1")
            .expect("should not error");

        match decision {
            SchedulerDecision::Backpressure { reason, .. } => {
                let msg = format!("{:?}", reason);
                assert!(
                    msg.contains("circuit breaker open"),
                    "unexpected reason: {}",
                    msg
                );
            }
            other => panic!("expected Backpressure, got {:?}", other),
        }
    }

    #[test]
    fn closed_circuit_breaker_allows_dispatch() {
        use crate::circuit_breaker::CircuitBreaker;
        use std::sync::Arc;

        let repo = FakeRepository::new(vec![attempt("attempt-a", 1)], &[]);
        let breaker = Arc::new(CircuitBreaker::new(30)); // starts Closed

        let scheduler = SkeletonScheduler::new(repo).with_circuit_breaker(breaker);

        let decision = scheduler
            .dispatch_one("worker-1")
            .expect("dispatch should succeed");

        assert!(
            matches!(decision, SchedulerDecision::Dispatched { .. }),
            "expected Dispatched, got {:?}",
            decision
        );
    }

    // ---------------------------------------------------------------------------
    // K5-c: Context-Aware Scheduler Tests
    // ---------------------------------------------------------------------------

    #[test]
    fn dispatch_with_priority_weighted_fairness() {
        let repo = FakeRepository::new(
            vec![
                attempt("attempt-low", 1),   // lower attempt_no = lower priority
                attempt("attempt-high", 10), // higher attempt_no = higher priority
            ],
            &[],
        );
        let scheduler = SkeletonScheduler::new(repo);
        let ctx = DispatchContext::new().with_fairness_policy(FairnessPolicy::PriorityWeighted {
            default_weight: 100,
        });

        let decision = scheduler
            .dispatch_one_with_context("worker-1", Some(&ctx))
            .expect("dispatch should succeed");

        match decision {
            SchedulerDecision::Dispatched { attempt_id, .. } => {
                // With PriorityWeighted, higher attempt_no should be dispatched first
                assert_eq!(attempt_id, "attempt-high");
            }
            other => panic!("expected Dispatched, got {:?}", other),
        }
    }

    #[test]
    fn dispatch_context_builder_with_priority_and_fairness() {
        let ctx = DispatchContext::new()
            .with_tenant("tenant-1")
            .with_priority(5)
            .with_fairness_policy(FairnessPolicy::RoundRobin)
            .with_thread_priority(ThreadPriority::HIGH)
            .with_resource_budget(ResourceBudget::unbounded());

        assert_eq!(ctx.tenant_id.as_deref(), Some("tenant-1"));
        assert_eq!(ctx.priority, Some(5));
        assert!(matches!(
            ctx.fairness_policy,
            Some(FairnessPolicy::RoundRobin)
        ));
        assert!(matches!(ctx.thread_priority, Some(ThreadPriority::HIGH)));
        assert!(ctx.resource_budget.is_some());
    }

    // ---------------------------------------------------------------------------
    // K5-d: Backpressure Engine Tests
    // ---------------------------------------------------------------------------

    #[test]
    fn per_worker_backpressure_tracks_lease_counts() {
        let repo = FakeRepository::new(vec![attempt("attempt-a", 1)], &[]);
        let throttle_limits = ThrottleLimits {
            max_concurrent_runs_per_tenant: 100,
            max_concurrent_leases_per_worker: 2,
            max_queue_depth: 1000,
        };

        let scheduler = SkeletonScheduler::new(repo).with_throttle_limits(throttle_limits.clone());

        // First dispatch should succeed
        let decision1 = scheduler.dispatch_one("worker-1").expect("dispatch 1");
        assert!(matches!(decision1, SchedulerDecision::Dispatched { .. }));

        // Second dispatch to same worker should also succeed (limit is 2)
        let decision2 = scheduler.dispatch_one("worker-1").expect("dispatch 2");
        assert!(matches!(decision2, SchedulerDecision::Dispatched { .. }));

        // Third dispatch should fail with backpressure
        let decision3 = scheduler.dispatch_one("worker-1").expect("dispatch 3");
        match decision3 {
            SchedulerDecision::Backpressure { reason, .. } => {
                let msg = format!("{:?}", reason);
                assert!(msg.contains("worker-1 at 2 leases"), "got: {}", msg);
            }
            other => panic!("expected Backpressure, got {:?}", other),
        }
    }

    #[test]
    fn scheduler_metrics_tracks_counts() {
        let repo = FakeRepository::new(vec![attempt("attempt-a", 1)], &[]);
        let scheduler = SkeletonScheduler::new(repo);

        let _ = scheduler.dispatch_one("worker-1").expect("dispatch");

        let metrics = scheduler.get_metrics();
        assert_eq!(metrics.worker_lease_counts.get("worker-1"), Some(&1));
    }

    #[test]
    fn throttle_limits_defaults() {
        let limits = ThrottleLimits::default();
        assert_eq!(limits.max_concurrent_runs_per_tenant, 100);
        assert_eq!(limits.max_concurrent_leases_per_worker, 10);
        assert_eq!(limits.max_queue_depth, 1000);
    }
}
