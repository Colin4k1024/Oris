//! Proposal-only runtime contract for external agents.

use serde::{Deserialize, Serialize};

pub const A2A_PROTOCOL_NAME: &str = "oris.a2a";
pub const A2A_PROTOCOL_VERSION: &str = "0.1.0-experimental";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct A2aProtocol {
    pub name: String,
    pub version: String,
}

impl A2aProtocol {
    pub fn current() -> Self {
        Self {
            name: A2A_PROTOCOL_NAME.to_string(),
            version: A2A_PROTOCOL_VERSION.to_string(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum A2aCapability {
    Coordination,
    MutationProposal,
    ReplayFeedback,
    SupervisedDevloop,
    EvolutionPublish,
    EvolutionFetch,
    EvolutionRevoke,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct A2aHandshakeRequest {
    pub agent_id: String,
    pub role: AgentRole,
    pub capability_level: AgentCapabilityLevel,
    pub supported_protocols: Vec<A2aProtocol>,
    pub advertised_capabilities: Vec<A2aCapability>,
}

impl A2aHandshakeRequest {
    pub fn supports_current_protocol(&self) -> bool {
        self.supported_protocols.iter().any(|protocol| {
            protocol.name == A2A_PROTOCOL_NAME && protocol.version == A2A_PROTOCOL_VERSION
        })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct A2aHandshakeResponse {
    pub accepted: bool,
    pub negotiated_protocol: Option<A2aProtocol>,
    pub enabled_capabilities: Vec<A2aCapability>,
    pub message: Option<String>,
    pub error: Option<A2aErrorEnvelope>,
}

impl A2aHandshakeResponse {
    pub fn accept(enabled_capabilities: Vec<A2aCapability>) -> Self {
        Self {
            accepted: true,
            negotiated_protocol: Some(A2aProtocol::current()),
            enabled_capabilities,
            message: Some("handshake accepted".to_string()),
            error: None,
        }
    }

    pub fn reject(code: A2aErrorCode, message: impl Into<String>, details: Option<String>) -> Self {
        Self {
            accepted: false,
            negotiated_protocol: None,
            enabled_capabilities: Vec::new(),
            message: Some("handshake rejected".to_string()),
            error: Some(A2aErrorEnvelope {
                code,
                message: message.into(),
                retriable: true,
                details,
            }),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum A2aTaskLifecycleState {
    Queued,
    Running,
    Succeeded,
    Failed,
    Cancelled,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct A2aTaskLifecycleEvent {
    pub task_id: String,
    pub state: A2aTaskLifecycleState,
    pub summary: String,
    pub updated_at_ms: u64,
    pub error: Option<A2aErrorEnvelope>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum A2aErrorCode {
    UnsupportedProtocol,
    UnsupportedCapability,
    ValidationFailed,
    AuthorizationDenied,
    Timeout,
    Internal,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct A2aErrorEnvelope {
    pub code: A2aErrorCode,
    pub message: String,
    pub retriable: bool,
    pub details: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
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
