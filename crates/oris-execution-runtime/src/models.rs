//! Runtime domain models for Phase 1 skeleton.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use oris_kernel::identity::{RunId, Seq};

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

/// Bounty status enum
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum BountyStatus {
    Open,
    Accepted,
    Closed,
}

impl BountyStatus {
    pub fn as_str(&self) -> &str {
        match self {
            BountyStatus::Open => "open",
            BountyStatus::Accepted => "accepted",
            BountyStatus::Closed => "closed",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "accepted" => BountyStatus::Accepted,
            "closed" => BountyStatus::Closed,
            _ => BountyStatus::Open,
        }
    }
}

/// Bounty record for task rewards
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BountyRecord {
    pub bounty_id: String,
    pub title: String,
    pub description: Option<String>,
    pub reward: i64,
    pub status: BountyStatus,
    pub created_by: String,
    pub created_at_ms: i64,
    pub closed_at_ms: Option<i64>,
    pub accepted_by: Option<String>,
    pub accepted_at_ms: Option<i64>,
}

/// Swarm task decomposition record
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SwarmTaskRecord {
    pub parent_task_id: String,
    pub decomposition_json: String,
    pub proposer_id: String,
    pub proposer_reward_pct: i32,
    pub solver_reward_pct: i32,
    pub aggregator_reward_pct: i32,
    pub status: String,
    pub created_at_ms: i64,
    pub completed_at_ms: Option<i64>,
}

/// Worker registration record
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorkerRecord {
    pub worker_id: String,
    pub domains: String,
    pub max_load: i32,
    pub metadata_json: Option<String>,
    pub registered_at_ms: i64,
    pub last_heartbeat_ms: Option<i64>,
    pub status: String,
}

/// Recipe record for reusable gene sequences
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RecipeRecord {
    pub recipe_id: String,
    pub name: String,
    pub description: Option<String>,
    pub gene_sequence_json: String,
    pub author_id: String,
    pub forked_from: Option<String>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    pub is_public: bool,
}

/// Organism record for running recipes
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OrganismRecord {
    pub organism_id: String,
    pub recipe_id: String,
    pub status: String,
    pub current_step: i32,
    pub total_steps: i32,
    pub created_at_ms: i64,
    pub completed_at_ms: Option<i64>,
}

/// Session record for collaborative sessions
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SessionRecord {
    pub session_id: String,
    pub session_type: String,
    pub creator_id: String,
    pub status: String,
    pub created_at_ms: i64,
    pub ended_at_ms: Option<i64>,
}

/// Session message record
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SessionMessageRecord {
    pub message_id: String,
    pub session_id: String,
    pub sender_id: String,
    pub content: String,
    pub message_type: String,
    pub sent_at_ms: i64,
}

/// Dispute status enum
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum DisputeStatus {
    Open,
    Resolved,
}

impl DisputeStatus {
    pub fn as_str(&self) -> &str {
        match self {
            DisputeStatus::Open => "open",
            DisputeStatus::Resolved => "resolved",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "resolved" => DisputeStatus::Resolved,
            _ => DisputeStatus::Open,
        }
    }
}

/// Dispute record
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DisputeRecord {
    pub dispute_id: String,
    pub bounty_id: String,
    pub opened_by: String,
    pub status: DisputeStatus,
    pub evidence_json: Option<String>,
    pub resolution: Option<String>,
    pub resolved_by: Option<String>,
    pub resolved_at_ms: Option<i64>,
    pub created_at_ms: i64,
}
