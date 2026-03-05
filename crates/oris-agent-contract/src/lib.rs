//! Proposal-only runtime contract for external agents.

use serde::{Deserialize, Serialize};

pub const A2A_PROTOCOL_NAME: &str = "oris.a2a";
pub const A2A_PROTOCOL_VERSION: &str = "0.1.0-experimental";
pub const A2A_PROTOCOL_VERSION_V1: &str = "1.0.0";
pub const A2A_SUPPORTED_PROTOCOL_VERSIONS: [&str; 2] =
    [A2A_PROTOCOL_VERSION_V1, A2A_PROTOCOL_VERSION];

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
    pub fn supports_protocol_version(&self, version: &str) -> bool {
        self.supported_protocols
            .iter()
            .any(|protocol| protocol.name == A2A_PROTOCOL_NAME && protocol.version == version)
    }

    pub fn supports_current_protocol(&self) -> bool {
        self.supports_protocol_version(A2A_PROTOCOL_VERSION)
    }

    pub fn negotiate_supported_protocol(&self) -> Option<A2aProtocol> {
        for version in A2A_SUPPORTED_PROTOCOL_VERSIONS {
            if self.supports_protocol_version(version) {
                return Some(A2aProtocol {
                    name: A2A_PROTOCOL_NAME.to_string(),
                    version: version.to_string(),
                });
            }
        }
        None
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

pub const A2A_TASK_SESSION_PROTOCOL_VERSION: &str = A2A_PROTOCOL_VERSION;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum A2aTaskSessionState {
    Started,
    Dispatched,
    InProgress,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct A2aTaskSessionStartRequest {
    pub sender_id: String,
    pub protocol_version: String,
    pub task_id: String,
    pub task_summary: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct A2aTaskSessionDispatchRequest {
    pub sender_id: String,
    pub protocol_version: String,
    pub dispatch_id: String,
    pub summary: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct A2aTaskSessionProgressRequest {
    pub sender_id: String,
    pub protocol_version: String,
    pub progress_pct: u8,
    pub summary: String,
    pub retryable: bool,
    pub retry_after_ms: Option<u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct A2aTaskSessionCompletionRequest {
    pub sender_id: String,
    pub protocol_version: String,
    pub terminal_state: A2aTaskLifecycleState,
    pub summary: String,
    pub retryable: bool,
    pub retry_after_ms: Option<u64>,
    pub failure_code: Option<A2aErrorCode>,
    pub failure_details: Option<String>,
    pub used_capsule: bool,
    pub capsule_id: Option<String>,
    pub reasoning_steps_avoided: u64,
    pub fallback_reason: Option<String>,
    pub task_class_id: String,
    pub task_label: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct A2aTaskSessionProgressItem {
    pub progress_pct: u8,
    pub summary: String,
    pub retryable: bool,
    pub retry_after_ms: Option<u64>,
    pub updated_at_ms: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct A2aTaskSessionAck {
    pub session_id: String,
    pub task_id: String,
    pub state: A2aTaskSessionState,
    pub summary: String,
    pub retryable: bool,
    pub retry_after_ms: Option<u64>,
    pub updated_at_ms: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct A2aTaskSessionResult {
    pub terminal_state: A2aTaskLifecycleState,
    pub summary: String,
    pub retryable: bool,
    pub retry_after_ms: Option<u64>,
    pub failure_code: Option<A2aErrorCode>,
    pub failure_details: Option<String>,
    pub replay_feedback: ReplayFeedback,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct A2aTaskSessionCompletionResponse {
    pub ack: A2aTaskSessionAck,
    pub result: A2aTaskSessionResult,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct A2aTaskSessionSnapshot {
    pub session_id: String,
    pub sender_id: String,
    pub task_id: String,
    pub protocol_version: String,
    pub state: A2aTaskSessionState,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    pub dispatch_ids: Vec<String>,
    pub progress: Vec<A2aTaskSessionProgressItem>,
    pub result: Option<A2aTaskSessionResult>,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn handshake_request_with_versions(versions: &[&str]) -> A2aHandshakeRequest {
        A2aHandshakeRequest {
            agent_id: "agent-test".into(),
            role: AgentRole::Planner,
            capability_level: AgentCapabilityLevel::A2,
            supported_protocols: versions
                .iter()
                .map(|version| A2aProtocol {
                    name: A2A_PROTOCOL_NAME.into(),
                    version: (*version).into(),
                })
                .collect(),
            advertised_capabilities: vec![A2aCapability::Coordination],
        }
    }

    #[test]
    fn negotiate_supported_protocol_prefers_v1_when_available() {
        let req = handshake_request_with_versions(&[A2A_PROTOCOL_VERSION, A2A_PROTOCOL_VERSION_V1]);
        let negotiated = req
            .negotiate_supported_protocol()
            .expect("expected protocol negotiation success");
        assert_eq!(negotiated.name, A2A_PROTOCOL_NAME);
        assert_eq!(negotiated.version, A2A_PROTOCOL_VERSION_V1);
    }

    #[test]
    fn negotiate_supported_protocol_falls_back_to_experimental() {
        let req = handshake_request_with_versions(&[A2A_PROTOCOL_VERSION]);
        let negotiated = req
            .negotiate_supported_protocol()
            .expect("expected protocol negotiation success");
        assert_eq!(negotiated.version, A2A_PROTOCOL_VERSION);
    }

    #[test]
    fn negotiate_supported_protocol_returns_none_without_overlap() {
        let req = handshake_request_with_versions(&["0.0.1"]);
        assert!(req.negotiate_supported_protocol().is_none());
    }
}
