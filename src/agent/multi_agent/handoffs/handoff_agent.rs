use std::collections::HashMap;
use std::sync::Arc;

use crate::{
    agent::multi_agent::handoffs::HandoffTool,
    agent::{AgentError, UnifiedAgent},
    chain::ChainError,
    schemas::messages::Message,
    tools::Tool,
};

/// A wrapper that manages multiple agents and routes based on handoff state.
///
/// This implements the Handoffs pattern where agents can transfer control
/// to each other via tool calls.
pub struct HandoffAgent {
    /// Map of agent names to agent instances
    agents: HashMap<String, Arc<UnifiedAgent>>,
    /// Default agent to use when no active agent is set
    default_agent: Option<Arc<UnifiedAgent>>,
    /// The handoff tool to add to agents
    handoff_tool: Arc<HandoffTool>,
}

impl HandoffAgent {
    /// Create a new HandoffAgent
    pub fn new() -> Self {
        Self {
            agents: HashMap::new(),
            default_agent: None,
            handoff_tool: Arc::new(HandoffTool::new()),
        }
    }

    /// Add an agent to the handoff system
    pub fn with_agent(mut self, name: String, agent: Arc<UnifiedAgent>) -> Self {
        self.agents.insert(name, agent);
        self
    }

    /// Set the default agent (used when no active agent is specified)
    pub fn with_default_agent(mut self, agent: Arc<UnifiedAgent>) -> Self {
        self.default_agent = Some(agent);
        self
    }

    /// Get the handoff tool (to add to agents)
    pub fn handoff_tool(&self) -> Arc<dyn Tool> {
        self.handoff_tool.clone()
    }

    /// Get an agent by name
    pub fn get_agent(&self, name: &str) -> Option<&Arc<UnifiedAgent>> {
        self.agents.get(name)
    }

    /// Get the default agent
    pub fn default_agent(&self) -> Option<&Arc<UnifiedAgent>> {
        self.default_agent.as_ref()
    }

    /// Invoke with messages, automatically routing based on state
    pub async fn invoke_messages(&self, messages: Vec<Message>) -> Result<String, ChainError> {
        // Extract the last human message to check for handoff instructions
        let _last_human_message = messages
            .iter()
            .rev()
            .find(|m| matches!(m.message_type, crate::schemas::MessageType::HumanMessage));

        // For now, we'll use the default agent or first agent
        // In a full implementation, this would check the state for active_agent
        let agent = self
            .default_agent
            .as_ref()
            .or_else(|| self.agents.values().next())
            .ok_or_else(|| ChainError::AgentError("No agent available for handoff".to_string()))?;

        agent.invoke_messages(messages).await
    }
}

impl Default for HandoffAgent {
    fn default() -> Self {
        Self::new()
    }
}

/// Builder for creating agents with handoff support
pub struct HandoffAgentBuilder {
    /// Base agent to add handoff capability to
    base_agent: Option<Arc<UnifiedAgent>>,
    /// Additional agents that can be handed off to
    handoff_agents: HashMap<String, Arc<UnifiedAgent>>,
}

impl HandoffAgentBuilder {
    /// Create a new HandoffAgentBuilder
    pub fn new() -> Self {
        Self {
            base_agent: None,
            handoff_agents: HashMap::new(),
        }
    }

    /// Set the base agent (will have handoff tool added)
    pub fn with_base_agent(mut self, agent: Arc<UnifiedAgent>) -> Self {
        self.base_agent = Some(agent);
        self
    }

    /// Add an agent that can be handed off to
    pub fn with_handoff_agent(mut self, name: String, agent: Arc<UnifiedAgent>) -> Self {
        self.handoff_agents.insert(name, agent);
        self
    }

    /// Build the handoff system
    pub fn build(self) -> Result<HandoffAgent, AgentError> {
        let mut handoff_agent = HandoffAgent::new();

        // Add all handoff agents
        for (name, agent) in self.handoff_agents {
            handoff_agent = handoff_agent.with_agent(name, agent);
        }

        // Set default agent if provided
        if let Some(base) = self.base_agent {
            handoff_agent = handoff_agent.with_default_agent(base);
        }

        Ok(handoff_agent)
    }
}

impl Default for HandoffAgentBuilder {
    fn default() -> Self {
        Self::new()
    }
}
