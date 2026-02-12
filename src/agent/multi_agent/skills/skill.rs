use async_trait::async_trait;

/// Context loaded by a skill, containing prompts, documents, or other information
#[derive(Clone, Debug)]
pub struct SkillContext {
    /// The context content (could be prompts, documents, etc.)
    pub content: String,
    /// Optional metadata about the context
    pub metadata: Option<std::collections::HashMap<String, String>>,
}

impl SkillContext {
    /// Create a new SkillContext
    pub fn new(content: String) -> Self {
        Self {
            content,
            metadata: None,
        }
    }

    /// Create with metadata
    pub fn with_metadata(
        content: String,
        metadata: std::collections::HashMap<String, String>,
    ) -> Self {
        Self {
            content,
            metadata: Some(metadata),
        }
    }
}

/// Trait for defining skills that can be loaded on-demand.
///
/// Skills provide specialized prompts and knowledge that can be
/// dynamically loaded into an agent's context.
#[async_trait]
pub trait Skill: Send + Sync {
    /// Get the name of the skill
    fn name(&self) -> String;

    /// Get a description of what this skill does
    fn description(&self) -> String;

    /// Load the skill context (prompts, documents, etc.)
    ///
    /// This method is called when the skill needs to be loaded.
    /// It should return the context that will be injected into the agent's prompt.
    async fn load_context(&self) -> Result<SkillContext, Box<dyn std::error::Error>>;

    /// Check if this skill should be loaded based on the input
    ///
    /// This is an optional method that can be used to determine
    /// if a skill should be automatically loaded based on the user input.
    fn should_load(&self, _input: &str) -> bool {
        false
    }
}

/// A simple skill implementation that provides static context
pub struct SimpleSkill {
    name: String,
    description: String,
    context: SkillContext,
}

impl SimpleSkill {
    /// Create a new SimpleSkill
    pub fn new(name: String, description: String, context: SkillContext) -> Self {
        Self {
            name,
            description,
            context,
        }
    }

    /// Create with a simple string context
    pub fn with_context(name: String, description: String, context: String) -> Self {
        Self {
            name,
            description,
            context: SkillContext::new(context),
        }
    }
}

#[async_trait]
impl Skill for SimpleSkill {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn description(&self) -> String {
        self.description.clone()
    }

    async fn load_context(&self) -> Result<SkillContext, Box<dyn std::error::Error>> {
        Ok(self.context.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_simple_skill() {
        let skill = SimpleSkill::with_context(
            "test_skill".to_string(),
            "A test skill".to_string(),
            "Test context content".to_string(),
        );

        assert_eq!(skill.name(), "test_skill");
        assert_eq!(skill.description(), "A test skill");

        let context = skill.load_context().await.unwrap();
        assert_eq!(context.content, "Test context content");
    }

    #[test]
    fn test_skill_context() {
        let context = SkillContext::new("Test content".to_string());
        assert_eq!(context.content, "Test content");
        assert!(context.metadata.is_none());
    }
}
