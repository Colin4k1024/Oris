use std::sync::Arc;

use crate::{
    agent::multi_agent::subagents::SubagentTool,
    agent::{create_agent, AgentError, UnifiedAgent},
    tools::Tool,
};

/// Information about a subagent to be added to the main agent
pub struct SubagentInfo {
    /// The subagent instance
    pub agent: Arc<UnifiedAgent>,
    /// Name of the subagent (used as tool name)
    pub name: String,
    /// Description of what this subagent does
    pub description: String,
}

impl SubagentInfo {
    /// Create a new SubagentInfo
    pub fn new(agent: Arc<UnifiedAgent>, name: String, description: String) -> Self {
        Self {
            agent,
            name,
            description,
        }
    }
}

/// Builder for creating a main agent with subagents.
///
/// This implements the Subagents pattern where a main agent coordinates
/// subagents as tools.
pub struct SubagentsBuilder {
    /// Model string for the main agent
    model: Option<String>,
    /// System prompt for the main agent
    system_prompt: Option<String>,
    /// Regular tools (non-agent tools) for the main agent
    tools: Vec<Arc<dyn Tool>>,
    /// Subagents to be added as tools
    subagents: Vec<SubagentInfo>,
}

impl SubagentsBuilder {
    /// Create a new SubagentsBuilder
    pub fn new() -> Self {
        Self {
            model: None,
            system_prompt: None,
            tools: Vec::new(),
            subagents: Vec::new(),
        }
    }

    /// Set the model for the main agent
    pub fn with_model<S: Into<String>>(mut self, model: S) -> Self {
        self.model = Some(model.into());
        self
    }

    /// Set the system prompt for the main agent
    pub fn with_system_prompt<S: Into<String>>(mut self, system_prompt: S) -> Self {
        self.system_prompt = Some(system_prompt.into());
        self
    }

    /// Add regular tools to the main agent
    pub fn with_tools(mut self, tools: &[Arc<dyn Tool>]) -> Self {
        self.tools.extend_from_slice(tools);
        self
    }

    /// Add a subagent
    pub fn with_subagent(mut self, subagent: SubagentInfo) -> Self {
        self.subagents.push(subagent);
        self
    }

    /// Add multiple subagents
    pub fn with_subagents(mut self, subagents: Vec<SubagentInfo>) -> Self {
        self.subagents.extend(subagents);
        self
    }

    /// Build the main agent with subagents as tools
    pub fn build(self) -> Result<UnifiedAgent, AgentError> {
        let model = self
            .model
            .ok_or_else(|| AgentError::MissingObject("model".to_string()))?;

        // Convert subagents to tools
        let mut all_tools: Vec<Arc<dyn Tool>> = self.tools;
        for subagent_info in self.subagents {
            let subagent_tool = Arc::new(SubagentTool::new(
                subagent_info.agent,
                subagent_info.name,
                subagent_info.description,
            ));
            all_tools.push(subagent_tool);
        }

        // Create the main agent with all tools (including subagents)
        let system_prompt = self.system_prompt.unwrap_or_else(|| {
            "You are a helpful assistant that coordinates specialized subagents to help users."
                .to_string()
        });

        create_agent(&model, &all_tools, Some(&system_prompt), None)
    }
}

impl Default for SubagentsBuilder {
    fn default() -> Self {
        Self::new()
    }
}
