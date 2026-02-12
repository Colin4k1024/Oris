use std::collections::HashMap;
use std::sync::Arc;

use crate::{
    agent::multi_agent::router::{DefaultRouter, Router},
    agent::{AgentError, UnifiedAgent},
    chain::ChainError,
    schemas::messages::Message,
};

/// An agent wrapper that routes input to specialized agents.
///
/// This implements the Router pattern where a routing step classifies
/// input and directs it to one or more specialized agents.
pub struct RouterAgent {
    /// Map of agent names to agent instances
    agents: HashMap<String, Arc<UnifiedAgent>>,
    /// The router that determines which agent to use
    router: Box<dyn Router>,
    /// Default agent to use when routing fails
    default_agent: Option<Arc<UnifiedAgent>>,
    /// Whether to allow parallel execution of multiple agents
    allow_parallel: bool,
}

impl RouterAgent {
    /// Create a new RouterAgent
    pub fn new(router: Box<dyn Router>) -> Self {
        Self {
            agents: HashMap::new(),
            router,
            default_agent: None,
            allow_parallel: false,
        }
    }

    /// Add an agent to the router
    pub fn with_agent(mut self, name: String, agent: Arc<UnifiedAgent>) -> Self {
        self.agents.insert(name, agent);
        self
    }

    /// Add multiple agents
    pub fn with_agents(mut self, agents: Vec<(String, Arc<UnifiedAgent>)>) -> Self {
        for (name, agent) in agents {
            self.agents.insert(name, agent);
        }
        self
    }

    /// Set the default agent (used when routing fails)
    pub fn with_default_agent(mut self, agent: Arc<UnifiedAgent>) -> Self {
        self.default_agent = Some(agent);
        self
    }

    /// Enable parallel execution of multiple agents
    pub fn with_parallel_execution(mut self, allow: bool) -> Self {
        self.allow_parallel = allow;
        self
    }

    /// Get an agent by name
    pub fn get_agent(&self, name: &str) -> Option<&Arc<UnifiedAgent>> {
        self.agents.get(name)
    }

    /// Route and invoke the appropriate agent(s)
    pub async fn invoke_messages(&self, messages: Vec<Message>) -> Result<String, ChainError> {
        // Extract the last human message
        let last_human_message = messages
            .iter()
            .rev()
            .find(|m| matches!(m.message_type, crate::schemas::MessageType::HumanMessage))
            .ok_or_else(|| ChainError::AgentError("No human message found".to_string()))?;

        let input = &last_human_message.content;

        // Route to determine which agent(s) should handle this
        let selected_agent_name = self
            .router
            .route(input)
            .await
            .map_err(|e| ChainError::AgentError(e.to_string()))?;

        if let Some(agent_name) = selected_agent_name {
            // Get the selected agent
            let agent = self.agents.get(&agent_name).ok_or_else(|| {
                ChainError::AgentError(format!("Agent not found: {}", agent_name))
            })?;

            // Invoke the selected agent
            agent.invoke_messages(messages).await
        } else {
            // No agent selected, use default or return error
            if let Some(default) = &self.default_agent {
                default.invoke_messages(messages).await
            } else {
                Err(ChainError::AgentError(
                    "No suitable agent found and no default agent configured".to_string(),
                ))
            }
        }
    }
}

/// Builder for creating RouterAgent
pub struct RouterAgentBuilder {
    /// Router strategy
    router: Option<Box<dyn Router>>,
    /// Agents to add
    agents: Vec<(String, Arc<UnifiedAgent>)>,
    /// Default agent
    default_agent: Option<Arc<UnifiedAgent>>,
    /// Allow parallel execution
    allow_parallel: bool,
}

impl RouterAgentBuilder {
    /// Create a new RouterAgentBuilder
    pub fn new() -> Self {
        Self {
            router: None,
            agents: Vec::new(),
            default_agent: None,
            allow_parallel: false,
        }
    }

    /// Set the router
    pub fn with_router(mut self, router: Box<dyn Router>) -> Self {
        self.router = Some(router);
        self
    }

    /// Set router with LLM-based strategy
    pub fn with_llm_router(
        self,
        llm: Box<dyn crate::language_models::llm::LLM>,
        agent_descriptions: Vec<(String, String)>,
    ) -> Self {
        let router = Box::new(DefaultRouter::with_llm(llm, agent_descriptions));
        self.with_router(router)
    }

    /// Set router with keyword-based strategy
    pub fn with_keyword_router(
        self,
        keyword_map: std::collections::HashMap<String, Vec<String>>,
    ) -> Self {
        let router = Box::new(DefaultRouter::with_keywords(keyword_map));
        self.with_router(router)
    }

    /// Add an agent
    pub fn with_agent(mut self, name: String, agent: Arc<UnifiedAgent>) -> Self {
        self.agents.push((name, agent));
        self
    }

    /// Add multiple agents
    pub fn with_agents(mut self, agents: Vec<(String, Arc<UnifiedAgent>)>) -> Self {
        self.agents.extend(agents);
        self
    }

    /// Set default agent
    pub fn with_default_agent(mut self, agent: Arc<UnifiedAgent>) -> Self {
        self.default_agent = Some(agent);
        self
    }

    /// Enable parallel execution
    pub fn with_parallel_execution(mut self, allow: bool) -> Self {
        self.allow_parallel = allow;
        self
    }

    /// Build the RouterAgent
    pub fn build(self) -> Result<RouterAgent, AgentError> {
        let router = self
            .router
            .ok_or_else(|| AgentError::MissingObject("router".to_string()))?;

        let mut router_agent = RouterAgent::new(router);
        router_agent = router_agent.with_agents(self.agents);

        if let Some(default) = self.default_agent {
            router_agent = router_agent.with_default_agent(default);
        }

        if self.allow_parallel {
            router_agent = router_agent.with_parallel_execution(true);
        }

        Ok(router_agent)
    }
}

impl Default for RouterAgentBuilder {
    fn default() -> Self {
        Self::new()
    }
}
