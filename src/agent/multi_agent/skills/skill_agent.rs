use std::collections::HashMap;
use std::sync::Arc;

use crate::{
    agent::multi_agent::skills::Skill,
    agent::{AgentError, UnifiedAgent},
    chain::ChainError,
    schemas::messages::Message,
};

/// An agent wrapper that supports dynamic skill loading.
///
/// This implements the Skills pattern where specialized prompts and
/// knowledge can be loaded on-demand while a single agent stays in control.
pub struct SkillAgent {
    /// The base agent
    agent: Arc<UnifiedAgent>,
    /// Registered skills
    skills: HashMap<String, Arc<dyn Skill>>,
    /// System prompt template that includes skill context
    system_prompt_template: Option<String>,
}

impl SkillAgent {
    /// Create a new SkillAgent
    pub fn new(agent: Arc<UnifiedAgent>) -> Self {
        Self {
            agent,
            skills: HashMap::new(),
            system_prompt_template: None,
        }
    }

    /// Add a skill to the agent
    pub fn with_skill(mut self, skill: Arc<dyn Skill>) -> Self {
        let skill_name = skill.name();
        self.skills.insert(skill_name, skill);
        self
    }

    /// Add multiple skills
    pub fn with_skills(mut self, skills: Vec<Arc<dyn Skill>>) -> Self {
        for skill in skills {
            let skill_name = skill.name();
            self.skills.insert(skill_name, skill);
        }
        self
    }

    /// Set a system prompt template that can include skill context
    ///
    /// The template can use `{skills}` placeholder to inject loaded skills
    pub fn with_system_prompt_template<S: Into<String>>(mut self, template: S) -> Self {
        self.system_prompt_template = Some(template.into());
        self
    }

    /// Get a skill by name
    pub fn get_skill(&self, name: &str) -> Option<&Arc<dyn Skill>> {
        self.skills.get(name)
    }

    /// Load a skill and return its context
    pub async fn load_skill(
        &self,
        name: &str,
    ) -> Result<crate::agent::multi_agent::skills::SkillContext, Box<dyn std::error::Error>> {
        let skill = self
            .skills
            .get(name)
            .ok_or_else(|| format!("Skill not found: {}", name))?;
        skill.load_context().await
    }

    /// Invoke the agent with messages, automatically loading relevant skills
    pub async fn invoke_messages(&self, messages: Vec<Message>) -> Result<String, ChainError> {
        // Extract the last human message to check for skill triggers
        let last_human_message = messages
            .iter()
            .rev()
            .find(|m| matches!(m.message_type, crate::schemas::MessageType::HumanMessage));

        // Check if any skills should be loaded based on input
        let mut loaded_skills = Vec::new();
        if let Some(msg) = last_human_message {
            for (name, skill) in &self.skills {
                if skill.should_load(&msg.content) {
                    match self.load_skill(name).await {
                        Ok(context) => {
                            loaded_skills.push((name.clone(), context));
                        }
                        Err(e) => {
                            log::warn!("Failed to load skill {}: {}", name, e);
                        }
                    }
                }
            }
        }

        // If skills were loaded, inject them into the system prompt
        let mut final_messages = messages;
        if !loaded_skills.is_empty() {
            let skill_contexts: Vec<String> = loaded_skills
                .iter()
                .map(|(name, ctx)| format!("## {}\n{}", name, ctx.content))
                .collect();
            let skills_text = skill_contexts.join("\n\n");

            // Prepend system message with skill context
            let system_message = if let Some(template) = &self.system_prompt_template {
                template.replace("{skills}", &skills_text)
            } else {
                format!(
                    "You have access to the following specialized knowledge:\n\n{}",
                    skills_text
                )
            };

            final_messages.insert(0, Message::new_system_message(system_message));
        }

        // Invoke the base agent
        self.agent.invoke_messages(final_messages).await
    }

    /// Get the underlying agent
    pub fn agent(&self) -> &Arc<UnifiedAgent> {
        &self.agent
    }
}

/// Builder for creating SkillAgent
pub struct SkillAgentBuilder {
    /// Base agent
    agent: Option<Arc<UnifiedAgent>>,
    /// Skills to add
    skills: Vec<Arc<dyn Skill>>,
    /// System prompt template
    system_prompt_template: Option<String>,
}

impl SkillAgentBuilder {
    /// Create a new SkillAgentBuilder
    pub fn new() -> Self {
        Self {
            agent: None,
            skills: Vec::new(),
            system_prompt_template: None,
        }
    }

    /// Set the base agent
    pub fn with_agent(mut self, agent: Arc<UnifiedAgent>) -> Self {
        self.agent = Some(agent);
        self
    }

    /// Add a skill
    pub fn with_skill(mut self, skill: Arc<dyn Skill>) -> Self {
        self.skills.push(skill);
        self
    }

    /// Add multiple skills
    pub fn with_skills(mut self, skills: Vec<Arc<dyn Skill>>) -> Self {
        self.skills.extend(skills);
        self
    }

    /// Set system prompt template
    pub fn with_system_prompt_template<S: Into<String>>(mut self, template: S) -> Self {
        self.system_prompt_template = Some(template.into());
        self
    }

    /// Build the SkillAgent
    pub fn build(self) -> Result<SkillAgent, AgentError> {
        let agent = self
            .agent
            .ok_or_else(|| AgentError::MissingObject("agent".to_string()))?;

        let mut skill_agent = SkillAgent::new(agent);
        skill_agent = skill_agent.with_skills(self.skills);

        if let Some(template) = self.system_prompt_template {
            skill_agent = skill_agent.with_system_prompt_template(template);
        }

        Ok(skill_agent)
    }
}

impl Default for SkillAgentBuilder {
    fn default() -> Self {
        Self::new()
    }
}
