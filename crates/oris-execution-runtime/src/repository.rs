//! Storage façade for runtime scheduler/lease operations.

use chrono::{DateTime, Utc};

use oris_kernel::event::KernelError;
use oris_kernel::identity::{RunId, Seq};

use super::models::{
    AttemptDispatchRecord, BountyRecord, DisputeRecord, LeaseRecord, OrganismRecord,
    RecipeRecord, SessionMessageRecord, SessionRecord, SwarmTaskRecord, WorkerRecord,
};

/// Runtime repository contract used by scheduler and lease manager.
///
/// Implementations are responsible for making dispatch ownership transitions
/// explicit:
/// - `list_dispatchable_attempts` must preserve the repository's dispatch order.
/// - `upsert_lease` must atomically claim only dispatchable attempts and move
///   the attempt into leased ownership when the claim succeeds.
/// - `expire_leases_and_requeue` must reclaim only leases whose expiry is older
///   than the supplied stale cutoff, so callers can apply a heartbeat grace
///   window before requeueing work.
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
    fn expire_leases_and_requeue(&self, stale_before: DateTime<Utc>) -> Result<u64, KernelError>;

    /// Transition attempts that exceeded their configured execution timeout.
    fn transition_timed_out_attempts(&self, _now: DateTime<Utc>) -> Result<u64, KernelError> {
        Ok(0)
    }

    /// Returns latest persisted sequence for a run (used by replay wiring).
    fn latest_seq_for_run(&self, run_id: &RunId) -> Result<Seq, KernelError>;

    // ============== Bounty Methods ==============

    /// Create or update a bounty
    fn upsert_bounty(&self, bounty: &BountyRecord) -> Result<(), KernelError>;

    /// Get a bounty by ID
    fn get_bounty(&self, bounty_id: &str) -> Result<Option<BountyRecord>, KernelError>;

    /// List bounties by status
    fn list_bounties(&self, status: Option<&str>, limit: usize) -> Result<Vec<BountyRecord>, KernelError>;

    /// Accept a bounty (transition to accepted)
    fn accept_bounty(&self, bounty_id: &str, accepted_by: &str) -> Result<(), KernelError>;

    /// Close a bounty (transition to closed)
    fn close_bounty(&self, bounty_id: &str) -> Result<(), KernelError>;

    // ============== Swarm Methods ==============

    /// Create or update swarm task decomposition
    fn upsert_swarm_decomposition(&self, task: &SwarmTaskRecord) -> Result<(), KernelError>;

    /// Get swarm task decomposition
    fn get_swarm_decomposition(&self, parent_task_id: &str) -> Result<Option<SwarmTaskRecord>, KernelError>;

    // ============== Worker Methods ==============

    /// Register a worker
    fn register_worker(&self, worker: &WorkerRecord) -> Result<(), KernelError>;

    /// Get a worker by ID
    fn get_worker(&self, worker_id: &str) -> Result<Option<WorkerRecord>, KernelError>;

    /// List workers by domain and status
    fn list_workers(&self, domain: Option<&str>, status: Option<&str>, limit: usize) -> Result<Vec<WorkerRecord>, KernelError>;

    /// Update worker heartbeat
    fn heartbeat_worker(&self, worker_id: &str, heartbeat_at_ms: i64) -> Result<(), KernelError>;

    // ============== Recipe Methods ==============

    /// Create a recipe
    fn create_recipe(&self, recipe: &RecipeRecord) -> Result<(), KernelError>;

    /// Get a recipe by ID
    fn get_recipe(&self, recipe_id: &str) -> Result<Option<RecipeRecord>, KernelError>;

    /// Fork a recipe (create a copy with new ID)
    fn fork_recipe(&self, original_id: &str, new_id: &str, new_author: &str) -> Result<Option<RecipeRecord>, KernelError>;

    /// List recipes by author
    fn list_recipes(&self, author_id: Option<&str>, limit: usize) -> Result<Vec<RecipeRecord>, KernelError>;

    // ============== Organism Methods ==============

    /// Express a recipe as an organism
    fn express_organism(&self, organism: &OrganismRecord) -> Result<(), KernelError>;

    /// Get an organism by ID
    fn get_organism(&self, organism_id: &str) -> Result<Option<OrganismRecord>, KernelError>;

    /// Update organism status (step progression)
    fn update_organism(&self, organism_id: &str, current_step: i32, status: &str) -> Result<(), KernelError>;

    // ============== Session Methods ==============

    /// Create a collaborative session
    fn create_session(&self, session: &SessionRecord) -> Result<(), KernelError>;

    /// Get a session by ID
    fn get_session(&self, session_id: &str) -> Result<Option<SessionRecord>, KernelError>;

    /// Add a message to session history
    fn add_session_message(&self, message: &SessionMessageRecord) -> Result<(), KernelError>;

    /// Get session message history
    fn get_session_history(&self, session_id: &str, limit: usize) -> Result<Vec<SessionMessageRecord>, KernelError>;

    // ============== Dispute Methods ==============

    /// Open a dispute
    fn open_dispute(&self, dispute: &DisputeRecord) -> Result<(), KernelError>;

    /// Get a dispute by ID
    fn get_dispute(&self, dispute_id: &str) -> Result<Option<DisputeRecord>, KernelError>;

    /// Get disputes for a bounty
    fn get_disputes_for_bounty(&self, bounty_id: &str) -> Result<Vec<DisputeRecord>, KernelError>;

    /// Resolve a dispute
    fn resolve_dispute(&self, dispute_id: &str, resolution: &str, resolved_by: &str) -> Result<(), KernelError>;
}
