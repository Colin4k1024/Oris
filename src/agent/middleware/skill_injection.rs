//! Middleware that injects matched skill content into the conversation (progressive disclosure).
//!
//! When the agent receives a prompt, skills whose frontmatter matches the user message
//! are loaded and their full content is prepended as a system message to chat_history
//! for that turn. Injection happens only on the first plan of each turn (when there are
//! no intermediate steps yet).

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::json;

use crate::schemas::Message;

use super::{Middleware, MiddlewareContext, MiddlewareError};
use crate::agent::deep_agent::skills::{load_skill_full_content, match_skills, SkillMeta};
use crate::agent::runtime::RuntimeRequest;
use crate::schemas::agent::AgentAction;

/// Middleware that injects directory-based skill content (SKILL.md with frontmatter)
/// into the agent's context when the user message matches a skill's description.
///
/// Uses progressive disclosure: only matched skills' full content is loaded and
/// prepended as a "## Skills" system message at the start of the turn.
pub struct SkillsMiddleware {
    /// Skill index (name, description, path) built at agent creation.
    index: Vec<SkillMeta>,
}

impl SkillsMiddleware {
    /// Create middleware from a pre-built skill index (e.g. from [crate::agent::deep_agent::skills::load_skill_index]).
    pub fn new(index: Vec<SkillMeta>) -> Self {
        Self { index }
    }

    /// Extract the last user message text from plan input (from "input" or last human message in chat_history).
    fn get_user_message_text(input: &crate::prompt::PromptArgs) -> String {
        if let Some(v) = input.get("input") {
            if let Some(s) = v.as_str() {
                return s.to_string();
            }
        }
        if let Some(chat_history) = input.get("chat_history") {
            if let Some(arr) = chat_history.as_array() {
                for msg in arr.iter().rev() {
                    if let Some(role) = msg.get("message_type").and_then(|r| r.as_str()) {
                        if role == "human" {
                            if let Some(content) = msg.get("content").and_then(|c| c.as_str()) {
                                return content.to_string();
                            }
                        }
                    }
                }
            }
        }
        String::new()
    }

    /// Build the skills section from matched skills and prepend as system message to chat_history.
    fn inject_skills_into_input(
        &self,
        mut input: crate::prompt::PromptArgs,
        user_text: &str,
    ) -> Result<crate::prompt::PromptArgs, MiddlewareError> {
        let matched = match_skills(&self.index, user_text);
        if matched.is_empty() {
            return Ok(input);
        }

        let mut parts = Vec::new();
        for meta in &matched {
            match load_skill_full_content(meta) {
                Ok(body) => {
                    parts.push(format!("### {}\n\n{}", meta.name, body));
                }
                Err(e) => {
                    log::warn!("Failed to load skill {}: {}", meta.name, e);
                }
            }
        }
        if parts.is_empty() {
            return Ok(input);
        }

        let skills_block = format!("\n\n## Skills\n\n{}", parts.join("\n\n"));
        let skills_message = Message::new_system_message(&skills_block);

        let chat_history = input
            .get("chat_history")
            .and_then(|v| v.as_array())
            .map(|a| a.to_vec())
            .unwrap_or_default();

        let mut new_history = vec![json!(skills_message)];
        new_history.extend(chat_history);
        input.insert("chat_history".to_string(), json!(new_history));

        Ok(input)
    }
}

#[async_trait]
impl Middleware for SkillsMiddleware {
    async fn before_agent_plan_with_runtime(
        &self,
        request: &RuntimeRequest,
        steps: &[(AgentAction, String)],
        context: &mut MiddlewareContext,
    ) -> Result<Option<crate::prompt::PromptArgs>, MiddlewareError> {
        let _ = context;
        if self.index.is_empty() {
            return Ok(None);
        }
        // Only inject on the first plan of the turn (no steps yet) to avoid repeating the block every iteration.
        if !steps.is_empty() {
            return Ok(None);
        }

        let user_text = Self::get_user_message_text(&request.input);
        let modified = self.inject_skills_into_input(request.input.clone(), &user_text)?;
        Ok(Some(modified))
    }
}

/// Builder for [SkillsMiddleware] when you have a list of skill directories.
pub fn build_skills_middleware(
    skill_dirs: &[std::path::PathBuf],
) -> Result<Option<Arc<dyn Middleware>>, std::io::Error> {
    let index = crate::agent::deep_agent::skills::load_skill_index(skill_dirs)?;
    if index.is_empty() {
        return Ok(None);
    }
    Ok(Some(Arc::new(SkillsMiddleware::new(index))))
}
