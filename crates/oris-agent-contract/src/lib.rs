//! Proposal-only runtime contract for external agents.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum AgentCapabilityLevel {
    A0,
    A1,
    A2,
    A3,
    A4,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ProposalTarget {
    WorkspaceRoot,
    Paths(Vec<String>),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentTask {
    pub id: String,
    pub description: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum AgentRole {
    Planner,
    Coder,
    Repair,
    Optimizer,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum CoordinationPrimitive {
    Sequential,
    Parallel,
    Conditional,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct CoordinationTask {
    pub id: String,
    pub role: AgentRole,
    pub description: String,
    pub depends_on: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct CoordinationMessage {
    pub from_role: AgentRole,
    pub to_role: AgentRole,
    pub task_id: String,
    pub content: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct CoordinationPlan {
    pub root_goal: String,
    pub primitive: CoordinationPrimitive,
    pub tasks: Vec<CoordinationTask>,
    pub timeout_ms: u64,
    pub max_retries: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct CoordinationResult {
    pub completed_tasks: Vec<String>,
    pub failed_tasks: Vec<String>,
    pub messages: Vec<CoordinationMessage>,
    pub summary: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MutationProposal {
    pub intent: String,
    pub files: Vec<String>,
    pub expected_effect: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExecutionFeedback {
    pub accepted: bool,
    pub asset_state: Option<String>,
    pub summary: String,
}
