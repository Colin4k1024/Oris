use std::collections::HashMap;

use serde_json::Value;

use crate::schemas::messages::Message;

/// Mutable agent state that flows through execution.
///
/// This includes messages, custom fields, counters, and any other
/// state that tools may need to access or modify.
#[derive(Clone, Debug)]
pub struct AgentState {
    /// Conversation messages
    pub messages: Vec<Message>,
    /// Custom fields that can be set and accessed by tools
    pub custom_fields: HashMap<String, Value>,
    /// Structured output response (when using structured output)
    pub structured_response: Option<Value>,
    /// Active agent name (for Handoffs pattern)
    pub active_agent: Option<String>,
    /// Loaded skills (for Skills pattern)
    pub loaded_skills: Vec<String>,
    /// Routing history: (input, selected_agent) pairs (for Router pattern)
    pub routing_history: Vec<(String, String)>,
}

impl Default for AgentState {
    fn default() -> Self {
        Self {
            messages: Vec::new(),
            custom_fields: HashMap::new(),
            structured_response: None,
            active_agent: None,
            loaded_skills: Vec::new(),
            routing_history: Vec::new(),
        }
    }
}

impl AgentState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_messages(messages: Vec<Message>) -> Self {
        Self {
            messages,
            custom_fields: HashMap::new(),
            structured_response: None,
            active_agent: None,
            loaded_skills: Vec::new(),
            routing_history: Vec::new(),
        }
    }

    pub fn get_field(&self, key: &str) -> Option<&Value> {
        self.custom_fields.get(key)
    }

    pub fn set_field(&mut self, key: String, value: Value) {
        self.custom_fields.insert(key, value);
    }

    pub fn remove_field(&mut self, key: &str) -> Option<Value> {
        self.custom_fields.remove(key)
    }

    /// Get the active agent name (for Handoffs)
    pub fn get_active_agent(&self) -> Option<&String> {
        self.active_agent.as_ref()
    }

    /// Set the active agent name (for Handoffs)
    pub fn set_active_agent(&mut self, agent_name: String) {
        self.active_agent = Some(agent_name);
    }

    /// Clear the active agent
    pub fn clear_active_agent(&mut self) {
        self.active_agent = None;
    }

    /// Check if a skill is loaded
    pub fn has_skill(&self, skill_name: &str) -> bool {
        self.loaded_skills.contains(&skill_name.to_string())
    }

    /// Add a loaded skill
    pub fn add_skill(&mut self, skill_name: String) {
        if !self.loaded_skills.contains(&skill_name) {
            self.loaded_skills.push(skill_name);
        }
    }

    /// Remove a skill
    pub fn remove_skill(&mut self, skill_name: &str) {
        self.loaded_skills.retain(|s| s != skill_name);
    }

    /// Add a routing history entry
    pub fn add_routing_history(&mut self, input: String, selected_agent: String) {
        self.routing_history.push((input, selected_agent));
    }
}

/// Command pattern for updating agent state or controlling execution flow.
///
/// Tools can return commands to update state, remove messages, or control
/// the agent's execution flow.
#[derive(Debug, Clone)]
pub enum Command {
    /// Update custom state fields
    UpdateState { fields: HashMap<String, Value> },
    /// Remove specific messages by ID
    RemoveMessages { ids: Vec<String> },
    /// Clear all messages
    ClearMessages,
    /// Clear all state (messages and custom fields)
    ClearState,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agent_state() {
        let mut state = AgentState::new();

        // Test field operations
        state.set_field("test_key".to_string(), serde_json::json!("test_value"));
        assert_eq!(
            state.get_field("test_key"),
            Some(&serde_json::json!("test_value"))
        );

        let removed = state.remove_field("test_key");
        assert_eq!(removed, Some(serde_json::json!("test_value")));
        assert_eq!(state.get_field("test_key"), None);
    }

    #[test]
    fn test_agent_state_with_messages() {
        let messages = vec![
            Message::new_human_message("Hello"),
            Message::new_ai_message("Hi there!"),
        ];

        let state = AgentState::with_messages(messages.clone());
        assert_eq!(state.messages.len(), 2);
        assert_eq!(state.custom_fields.len(), 0);
    }

    #[test]
    fn test_command_creation() {
        let mut fields = HashMap::new();
        fields.insert("key1".to_string(), serde_json::json!("value1"));

        let command = Command::UpdateState { fields };
        match command {
            Command::UpdateState { fields } => {
                assert_eq!(fields.len(), 1);
            }
            _ => panic!("Wrong command type"),
        }
    }

    #[test]
    fn test_active_agent() {
        let mut state = AgentState::new();
        assert_eq!(state.get_active_agent(), None);

        state.set_active_agent("test_agent".to_string());
        assert_eq!(state.get_active_agent(), Some(&"test_agent".to_string()));

        state.clear_active_agent();
        assert_eq!(state.get_active_agent(), None);
    }

    #[test]
    fn test_skills() {
        let mut state = AgentState::new();
        assert!(!state.has_skill("rust"));

        state.add_skill("rust".to_string());
        assert!(state.has_skill("rust"));

        state.add_skill("python".to_string());
        assert_eq!(state.loaded_skills.len(), 2);

        state.remove_skill("rust");
        assert!(!state.has_skill("rust"));
        assert!(state.has_skill("python"));
    }

    #[test]
    fn test_routing_history() {
        let mut state = AgentState::new();
        assert_eq!(state.routing_history.len(), 0);

        state.add_routing_history("test input".to_string(), "agent1".to_string());
        assert_eq!(state.routing_history.len(), 1);
        assert_eq!(state.routing_history[0].0, "test input");
        assert_eq!(state.routing_history[0].1, "agent1");
    }
}
