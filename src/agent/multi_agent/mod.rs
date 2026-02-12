use thiserror::Error;

use crate::agent::AgentError;

/// Multi-agent specific error types
#[derive(Error, Debug)]
pub enum MultiAgentError {
    #[error("Agent error: {0}")]
    AgentError(#[from] AgentError),

    #[error("Agent not found: {0}")]
    AgentNotFound(String),

    #[error("Invalid agent configuration: {0}")]
    InvalidConfiguration(String),

    #[error("Routing error: {0}")]
    RoutingError(String),

    #[error("Skill error: {0}")]
    SkillError(String),

    #[error("Handoff error: {0}")]
    HandoffError(String),

    #[error("Serde json error: {0}")]
    SerdeJsonError(#[from] serde_json::Error),
}

pub mod handoffs;
pub mod router;
pub mod skills;
pub mod subagents;

// Re-export commonly used types
pub use handoffs::{HandoffAgent, HandoffAgentBuilder, HandoffTool};
pub use router::{DefaultRouter, Router, RouterAgent, RouterAgentBuilder, RoutingStrategy};
pub use skills::{SimpleSkill, Skill, SkillAgent, SkillAgentBuilder, SkillContext};
pub use subagents::{SubagentInfo, SubagentTool, SubagentsBuilder};
