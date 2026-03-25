//! Lease management for Phase 1: single-owner execution, expiry, and recovery.
//!
//! [WorkerLease] wraps [super::models::LeaseRecord] to strictly enforce single-owner
//! execution: call [WorkerLease::verify_owner] and [WorkerLease::is_expired] before
//! running work. Lease expiry and recovery are handled by [LeaseManager::tick]
//! (expire stale leases, requeue attempts); replay-restart is re-dispatch after requeue.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::circuit_breaker::CircuitBreaker;

use chrono::{DateTime, Duration, Utc};

use oris_kernel::event::KernelError;

use super::models::{LeaseRecord, LeaseTerminalState};
use super::repository::RuntimeRepository;

/// Strict single-owner execution guard for a lease. Verify ownership and expiry before executing.
#[derive(Clone, Debug)]
pub struct WorkerLease {
    record: LeaseRecord,
}

impl WorkerLease {
    /// Build a worker lease from a repository lease record (e.g. from `get_lease_for_attempt`).
    pub fn from_record(record: LeaseRecord) -> Self {
        Self { record }
    }

    /// Lease record for heartbeat or persistence.
    pub fn record(&self) -> &LeaseRecord {
        &self.record
    }

    pub fn lease_id(&self) -> &str {
        &self.record.lease_id
    }

    pub fn attempt_id(&self) -> &str {
        &self.record.attempt_id
    }

    pub fn worker_id(&self) -> &str {
        &self.record.worker_id
    }

    /// Returns the terminal state if set (K5-a).
    pub fn terminal_state(&self) -> Option<&LeaseTerminalState> {
        self.record.terminal_state.as_ref()
    }

    /// Returns true if the lease is in a terminal state (not Active) (K5-a).
    pub fn is_terminal(&self) -> bool {
        self.record.terminal_state.is_some()
    }

    /// Returns true if the lease has passed its expiry time (no heartbeat grace here).
    pub fn is_expired(&self, now: DateTime<Utc>) -> bool {
        now >= self.record.lease_expires_at
    }

    /// Enforce single-owner: returns `Ok(())` only if `worker_id` matches the lease owner.
    pub fn verify_owner(&self, worker_id: &str) -> Result<(), KernelError> {
        if self.record.worker_id != worker_id {
            return Err(KernelError::Driver(format!(
                "lease {} is owned by {}, not {}",
                self.record.lease_id, self.record.worker_id, worker_id
            )));
        }
        Ok(())
    }

    /// Returns `Ok(())` if the given worker owns the lease and it is not yet expired.
    pub fn check_execution_allowed(
        &self,
        worker_id: &str,
        now: DateTime<Utc>,
    ) -> Result<(), KernelError> {
        self.verify_owner(worker_id)?;
        // Check terminal state first (K5-a)
        if self.is_terminal() {
            return Err(KernelError::Driver(format!(
                "lease {} is in terminal state {:?}",
                self.record.lease_id, self.record.terminal_state
            )));
        }
        if self.is_expired(now) {
            return Err(KernelError::Driver(format!(
                "lease {} expired at {}",
                self.record.lease_id, self.record.lease_expires_at
            )));
        }
        Ok(())
    }

    /// Validates state transition to a terminal state (K5-a).
    /// Returns Ok(()) if the transition is valid, error otherwise.
    pub fn can_transition_to(&self, new_state: &LeaseTerminalState) -> Result<(), KernelError> {
        // Can always transition to terminal states from Active
        if self.record.terminal_state.is_none() {
            return Ok(());
        }
        // Once terminal, no further transitions allowed
        Err(KernelError::Driver(format!(
            "invalid state transition: cannot transition from {:?} to {:?}",
            self.record.terminal_state, new_state
        )))
    }
}

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

// ---------------------------------------------------------------------------
// WorkerHealthTracker — per-worker lease-expiry counter
// ---------------------------------------------------------------------------

/// Per-worker health statistics maintained by the scheduler or control plane.
#[derive(Clone, Debug, Default)]
pub struct WorkerHealth {
    /// Total number of times this worker's leases have been expired and requeued.
    pub lease_expiry_count: u64,
    /// Last time a heartbeat was observed from this worker (epoch ms).
    pub last_heartbeat_ms: Option<i64>,
    /// Whether this worker is currently quarantined (too many consecutive expirations).
    pub quarantined: bool,
}

/// Thread-safe tracker that records per-worker health statistics.
///
/// When `lease_expiry_count` for a worker reaches `quarantine_threshold`, the
/// worker is marked `quarantined = true`. A quarantined worker should not
/// receive new dispatch leases until it is explicitly cleared.
///
/// An optional [`CircuitBreaker`] can be wired in via
/// [`WorkerHealthTracker::with_circuit_breaker`]. When a worker is quarantined,
/// the breaker is tripped automatically.
#[derive(Clone, Default)]
pub struct WorkerHealthTracker {
    inner: Arc<Mutex<HashMap<String, WorkerHealth>>>,
    /// Number of consecutive lease expirations before a worker is quarantined.
    /// Defaults to 5.
    quarantine_threshold: u64,
    /// Optional circuit breaker to trip when a worker is quarantined.
    circuit_breaker: Option<Arc<CircuitBreaker>>,
}

impl WorkerHealthTracker {
    pub fn new(quarantine_threshold: u64) -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
            quarantine_threshold,
            circuit_breaker: None,
        }
    }

    /// Attach a shared circuit breaker to this tracker.
    ///
    /// The breaker will be tripped whenever a worker is quarantined.
    pub fn with_circuit_breaker(mut self, breaker: Arc<CircuitBreaker>) -> Self {
        self.circuit_breaker = Some(breaker);
        self
    }

    /// Record one lease expiry for `worker_id`.
    /// Returns `true` if the worker was just quarantined by this call.
    ///
    /// If a circuit breaker is attached, it is tripped when quarantine occurs.
    pub fn record_expiry(&self, worker_id: &str) -> bool {
        let just_quarantined = {
            let mut map = self.inner.lock().expect("worker health lock poisoned");
            let entry = map.entry(worker_id.to_string()).or_default();
            entry.lease_expiry_count += 1;
            if !entry.quarantined && entry.lease_expiry_count >= self.quarantine_threshold {
                entry.quarantined = true;
                true
            } else {
                false
            }
        };
        if just_quarantined {
            if let Some(cb) = &self.circuit_breaker {
                cb.trip();
            }
        }
        just_quarantined
    }

    /// Record a successful dispatch acknowledgement for `worker_id`.
    ///
    /// Clears quarantine, resets expiry counter, and resets the circuit breaker to `Closed`.
    pub fn record_success(&self, worker_id: &str) {
        {
            let mut map = self.inner.lock().expect("worker health lock poisoned");
            let entry = map.entry(worker_id.to_string()).or_default();
            entry.lease_expiry_count = 0;
            entry.quarantined = false;
        }
        if let Some(cb) = &self.circuit_breaker {
            cb.record_success();
        }
    }

    /// Record a heartbeat for `worker_id` and clear quarantine if set.
    pub fn record_heartbeat(&self, worker_id: &str, heartbeat_ms: i64) {
        let mut map = self.inner.lock().expect("worker health lock poisoned");
        let entry = map.entry(worker_id.to_string()).or_default();
        entry.last_heartbeat_ms = Some(heartbeat_ms);
        // A successful heartbeat resets the expiry counter and lifts quarantine.
        entry.lease_expiry_count = 0;
        entry.quarantined = false;
    }

    /// Returns `true` if the worker is currently quarantined.
    pub fn is_quarantined(&self, worker_id: &str) -> bool {
        self.inner
            .lock()
            .expect("worker health lock poisoned")
            .get(worker_id)
            .map(|h| h.quarantined)
            .unwrap_or(false)
    }

    /// Snapshot the health record for `worker_id`, or `None` if unknown.
    pub fn get(&self, worker_id: &str) -> Option<WorkerHealth> {
        self.inner
            .lock()
            .expect("worker health lock poisoned")
            .get(worker_id)
            .cloned()
    }

    /// Clear quarantine and reset expiry counter for `worker_id`.
    pub fn clear_quarantine(&self, worker_id: &str) {
        let mut map = self.inner.lock().expect("worker health lock poisoned");
        if let Some(entry) = map.get_mut(worker_id) {
            entry.quarantined = false;
            entry.lease_expiry_count = 0;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;
    use oris_kernel::identity::{RunId, Seq};

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

    #[test]
    fn worker_lease_verify_owner_accepts_owner() {
        let record = LeaseRecord {
            lease_id: "L1".to_string(),
            attempt_id: "A1".to_string(),
            worker_id: "W1".to_string(),
            lease_expires_at: Utc::now() + Duration::seconds(60),
            heartbeat_at: Utc::now(),
            version: 1,
            terminal_state: None,
            terminal_at: None,
        };
        let lease = WorkerLease::from_record(record);
        assert!(lease.verify_owner("W1").is_ok());
        assert!(lease.verify_owner("W2").is_err());
    }

    #[test]
    fn worker_lease_is_expired() {
        let now = Utc::now();
        let record = LeaseRecord {
            lease_id: "L1".to_string(),
            attempt_id: "A1".to_string(),
            worker_id: "W1".to_string(),
            lease_expires_at: now - Duration::seconds(1),
            heartbeat_at: now - Duration::seconds(2),
            version: 1,
            terminal_state: None,
            terminal_at: None,
        };
        let lease = WorkerLease::from_record(record);
        assert!(lease.is_expired(now));
        assert!(!lease.is_expired(now - Duration::seconds(2)));
    }

    #[test]
    fn worker_lease_check_execution_allowed() {
        let now = Utc::now();
        let record = LeaseRecord {
            lease_id: "L1".to_string(),
            attempt_id: "A1".to_string(),
            worker_id: "W1".to_string(),
            lease_expires_at: now + Duration::seconds(10),
            heartbeat_at: now,
            version: 1,
            terminal_state: None,
            terminal_at: None,
        };
        let lease = WorkerLease::from_record(record);
        assert!(lease.check_execution_allowed("W1", now).is_ok());
        assert!(lease.check_execution_allowed("W2", now).is_err());
        assert!(lease
            .check_execution_allowed("W1", now + Duration::seconds(11))
            .is_err());
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

    // -----------------------------------------------------------------------
    // WorkerHealthTracker tests
    // -----------------------------------------------------------------------

    #[test]
    fn worker_health_tracker_quarantines_after_threshold() {
        let tracker = WorkerHealthTracker::new(3);
        assert!(!tracker.is_quarantined("w1"));

        let just_quarantined = tracker.record_expiry("w1");
        assert!(!just_quarantined); // 1 < 3
        tracker.record_expiry("w1"); // 2 < 3
        let just_quarantined = tracker.record_expiry("w1"); // 3 >= 3
        assert!(just_quarantined);
        assert!(tracker.is_quarantined("w1"));
    }

    #[test]
    fn worker_health_tracker_heartbeat_clears_quarantine() {
        let tracker = WorkerHealthTracker::new(2);
        tracker.record_expiry("w1");
        tracker.record_expiry("w1");
        assert!(tracker.is_quarantined("w1"));

        tracker.record_heartbeat("w1", 1_700_000_000_000);
        assert!(!tracker.is_quarantined("w1"));

        let health = tracker.get("w1").expect("health record exists");
        assert_eq!(health.lease_expiry_count, 0);
        assert_eq!(health.last_heartbeat_ms, Some(1_700_000_000_000));
    }

    #[test]
    fn worker_health_tracker_clear_quarantine_explicit() {
        let tracker = WorkerHealthTracker::new(1);
        tracker.record_expiry("w1");
        assert!(tracker.is_quarantined("w1"));

        tracker.clear_quarantine("w1");
        assert!(!tracker.is_quarantined("w1"));
        assert_eq!(tracker.get("w1").map(|h| h.lease_expiry_count), Some(0));
    }

    #[test]
    fn worker_health_tracker_unknown_worker_not_quarantined() {
        let tracker = WorkerHealthTracker::new(3);
        assert!(!tracker.is_quarantined("unknown-worker"));
        assert!(tracker.get("unknown-worker").is_none());
    }

    // K5-a: Lease terminal state tests
    #[test]
    fn worker_lease_terminal_state_blocks_execution() {
        let now = Utc::now();
        let record = LeaseRecord {
            lease_id: "L1".to_string(),
            attempt_id: "A1".to_string(),
            worker_id: "W1".to_string(),
            lease_expires_at: now + Duration::seconds(10),
            heartbeat_at: now,
            version: 1,
            terminal_state: Some(LeaseTerminalState::Completed),
            terminal_at: Some(now),
        };
        let lease = WorkerLease::from_record(record);
        assert!(lease.is_terminal());
        assert!(lease.check_execution_allowed("W1", now).is_err());
    }

    #[test]
    fn worker_lease_state_transition_guards() {
        let now = Utc::now();

        // Active lease can transition to any terminal state
        let active_record = LeaseRecord {
            lease_id: "L1".to_string(),
            attempt_id: "A1".to_string(),
            worker_id: "W1".to_string(),
            lease_expires_at: now + Duration::seconds(10),
            heartbeat_at: now,
            version: 1,
            terminal_state: None,
            terminal_at: None,
        };
        let active_lease = WorkerLease::from_record(active_record);
        assert!(active_lease
            .can_transition_to(&LeaseTerminalState::Completed)
            .is_ok());
        assert!(active_lease
            .can_transition_to(&LeaseTerminalState::Failed)
            .is_ok());
        assert!(active_lease
            .can_transition_to(&LeaseTerminalState::Expired)
            .is_ok());
        assert!(active_lease
            .can_transition_to(&LeaseTerminalState::Cancelled)
            .is_ok());

        // Terminal lease cannot transition again
        let terminal_record = LeaseRecord {
            lease_id: "L2".to_string(),
            attempt_id: "A1".to_string(),
            worker_id: "W1".to_string(),
            lease_expires_at: now + Duration::seconds(10),
            heartbeat_at: now,
            version: 1,
            terminal_state: Some(LeaseTerminalState::Completed),
            terminal_at: Some(now),
        };
        let terminal_lease = WorkerLease::from_record(terminal_record);
        assert!(terminal_lease
            .can_transition_to(&LeaseTerminalState::Failed)
            .is_err());
    }

    // -----------------------------------------------------------------------
    // Issue #372: Finalization semantics tests
    // -----------------------------------------------------------------------

    #[test]
    fn finalization_all_terminal_states_block_execution() {
        let now = Utc::now();
        let terminal_states = vec![
            LeaseTerminalState::Completed,
            LeaseTerminalState::Failed,
            LeaseTerminalState::Expired,
            LeaseTerminalState::Cancelled,
        ];
        for state in terminal_states {
            let record = LeaseRecord {
                lease_id: format!("L-{:?}", state),
                attempt_id: "A1".to_string(),
                worker_id: "W1".to_string(),
                lease_expires_at: now + Duration::seconds(60),
                heartbeat_at: now,
                version: 1,
                terminal_state: Some(state),
                terminal_at: Some(now),
            };
            let lease = WorkerLease::from_record(record);
            assert!(
                lease.check_execution_allowed("W1", now).is_err(),
                "terminal state {:?} must block execution",
                lease.terminal_state()
            );
        }
    }

    #[test]
    fn finalization_terminal_lease_rejects_all_further_transitions() {
        let now = Utc::now();
        let terminal_states = vec![
            LeaseTerminalState::Completed,
            LeaseTerminalState::Failed,
            LeaseTerminalState::Expired,
            LeaseTerminalState::Cancelled,
        ];
        for from_state in &terminal_states {
            let record = LeaseRecord {
                lease_id: "L1".to_string(),
                attempt_id: "A1".to_string(),
                worker_id: "W1".to_string(),
                lease_expires_at: now + Duration::seconds(10),
                heartbeat_at: now,
                version: 1,
                terminal_state: Some(from_state.clone()),
                terminal_at: Some(now),
            };
            let lease = WorkerLease::from_record(record);
            for to_state in &terminal_states {
                assert!(
                    lease.can_transition_to(to_state).is_err(),
                    "transition from {:?} to {:?} must be rejected",
                    from_state,
                    to_state
                );
            }
        }
    }

    #[test]
    fn finalization_active_lease_accepts_all_terminal_transitions() {
        let now = Utc::now();
        let record = LeaseRecord {
            lease_id: "L1".to_string(),
            attempt_id: "A1".to_string(),
            worker_id: "W1".to_string(),
            lease_expires_at: now + Duration::seconds(10),
            heartbeat_at: now,
            version: 1,
            terminal_state: None,
            terminal_at: None,
        };
        let lease = WorkerLease::from_record(record);
        let all_terminal = vec![
            LeaseTerminalState::Completed,
            LeaseTerminalState::Failed,
            LeaseTerminalState::Expired,
            LeaseTerminalState::Cancelled,
        ];
        for state in &all_terminal {
            assert!(
                lease.can_transition_to(state).is_ok(),
                "active lease must accept transition to {:?}",
                state
            );
        }
    }

    #[test]
    fn finalization_idempotent_cleanup_double_terminal_rejected() {
        let now = Utc::now();
        let active_record = LeaseRecord {
            lease_id: "L1".to_string(),
            attempt_id: "A1".to_string(),
            worker_id: "W1".to_string(),
            lease_expires_at: now + Duration::seconds(10),
            heartbeat_at: now,
            version: 1,
            terminal_state: None,
            terminal_at: None,
        };
        let active_lease = WorkerLease::from_record(active_record);
        assert!(active_lease
            .can_transition_to(&LeaseTerminalState::Completed)
            .is_ok());

        let terminal_record = LeaseRecord {
            lease_id: "L1".to_string(),
            attempt_id: "A1".to_string(),
            worker_id: "W1".to_string(),
            lease_expires_at: now + Duration::seconds(10),
            heartbeat_at: now,
            version: 2,
            terminal_state: Some(LeaseTerminalState::Completed),
            terminal_at: Some(now),
        };
        let terminal_lease = WorkerLease::from_record(terminal_record);
        assert!(terminal_lease
            .can_transition_to(&LeaseTerminalState::Completed)
            .is_err());
        assert!(terminal_lease
            .can_transition_to(&LeaseTerminalState::Failed)
            .is_err());
        assert!(terminal_lease.check_execution_allowed("W1", now).is_err());
    }

    #[test]
    fn finalization_terminal_state_is_none_for_active_lease() {
        let now = Utc::now();
        let record = LeaseRecord {
            lease_id: "L1".to_string(),
            attempt_id: "A1".to_string(),
            worker_id: "W1".to_string(),
            lease_expires_at: now + Duration::seconds(10),
            heartbeat_at: now,
            version: 1,
            terminal_state: None,
            terminal_at: None,
        };
        let lease = WorkerLease::from_record(record);
        assert!(!lease.is_terminal());
        assert!(lease.terminal_state().is_none());
        assert!(lease.check_execution_allowed("W1", now).is_ok());
    }
}
