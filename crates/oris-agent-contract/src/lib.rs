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

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExecutionFeedback {
    pub accepted: bool,
    pub asset_state: Option<String>,
    pub summary: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum ReplayPlannerDirective {
    SkipPlanner,
    PlanFallback,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReplayFeedback {
    pub used_capsule: bool,
    pub capsule_id: Option<String>,
    pub planner_directive: ReplayPlannerDirective,
    pub reasoning_steps_avoided: u64,
    pub fallback_reason: Option<String>,
    pub task_class_id: String,
    pub task_label: String,
    pub summary: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum BoundedTaskClass {
    DocsSingleFile,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct HumanApproval {
    pub approved: bool,
    pub approver: Option<String>,
    pub note: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SupervisedDevloopRequest {
    pub task: AgentTask,
    pub proposal: MutationProposal,
    pub approval: HumanApproval,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum SupervisedDevloopStatus {
    AwaitingApproval,
    RejectedByPolicy,
    Executed,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SupervisedDevloopOutcome {
    pub task_id: String,
    pub task_class: Option<BoundedTaskClass>,
    pub status: SupervisedDevloopStatus,
    pub execution_feedback: Option<ExecutionFeedback>,
    pub summary: String,
}
