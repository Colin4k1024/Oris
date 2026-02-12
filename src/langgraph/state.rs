use std::collections::HashMap;

use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;

use crate::schemas::messages::Message;

/// Trait for state types used in LangGraph
///
/// States must be cloneable, thread-safe, serializable, and must implement
/// a merge strategy for combining state updates.
pub trait State: Clone + Send + Sync + Serialize + DeserializeOwned {
    /// Merge another state into this state
    ///
    /// This method defines how state updates are combined.
    /// The default implementation should handle the most common case
    /// of merging state updates.
    fn merge(&self, other: &Self) -> Self;
}

/// State update type - a map of field names to values
///
/// Nodes return StateUpdate instead of full state objects.
/// The graph executor will merge these updates into the current state.
pub type StateUpdate = HashMap<String, Value>;

/// MessagesState - a state type containing only messages
///
/// This is the most common state type for LangGraph workflows,
/// similar to Python's MessagesState.
///
/// # Example
///
/// ```rust,no_run
/// use oris::langgraph::{MessagesState, State};
/// use oris::schemas::messages::Message;
///
/// let state = MessagesState {
///     messages: vec![Message::new_human_message("Hello")],
/// };
/// ```
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct MessagesState {
    /// List of messages in the conversation
    pub messages: Vec<Message>,
}

impl MessagesState {
    /// Create a new empty MessagesState
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a MessagesState with initial messages
    pub fn with_messages(messages: Vec<Message>) -> Self {
        Self { messages }
    }
}

impl State for MessagesState {
    fn merge(&self, other: &Self) -> Self {
        // For MessagesState, we append messages (append strategy)
        let mut messages = self.messages.clone();
        messages.extend(other.messages.clone());
        Self { messages }
    }
}

/// Helper function to create a state update from a MessagesState
pub fn messages_state_update(messages: Vec<Message>) -> StateUpdate {
    let mut update = HashMap::new();
    update.insert(
        "messages".to_string(),
        serde_json::to_value(messages).unwrap_or(Value::Array(vec![])),
    );
    update
}

/// Helper function to extract messages from a state update
pub fn extract_messages_from_update(update: &StateUpdate) -> Vec<Message> {
    update
        .get("messages")
        .and_then(|v| {
            if let Some(arr) = v.as_array() {
                arr.iter()
                    .filter_map(|item| serde_json::from_value::<Message>(item.clone()).ok())
                    .collect::<Vec<_>>()
                    .into()
            } else {
                None
            }
        })
        .unwrap_or_default()
}

/// Apply a state update to a MessagesState
pub fn apply_update_to_messages_state(
    state: &MessagesState,
    update: &StateUpdate,
) -> MessagesState {
    let mut new_state = state.clone();

    if let Some(messages_value) = update.get("messages") {
        if let Ok(new_messages) = serde_json::from_value::<Vec<Message>>(messages_value.clone()) {
            // Append strategy: add new messages to existing ones
            new_state.messages.extend(new_messages);
        }
    }

    new_state
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_messages_state_merge() {
        let state1 = MessagesState {
            messages: vec![Message::new_human_message("Hello")],
        };
        let state2 = MessagesState {
            messages: vec![Message::new_ai_message("Hi there!")],
        };

        let merged = state1.merge(&state2);
        assert_eq!(merged.messages.len(), 2);
        assert_eq!(merged.messages[0].content, "Hello");
        assert_eq!(merged.messages[1].content, "Hi there!");
    }

    #[test]
    fn test_messages_state_update() {
        let messages = vec![
            Message::new_human_message("Hello"),
            Message::new_ai_message("Hi!"),
        ];
        let update = messages_state_update(messages.clone());

        assert!(update.contains_key("messages"));
        let extracted = extract_messages_from_update(&update);
        assert_eq!(extracted.len(), 2);
    }

    #[test]
    fn test_apply_update() {
        let state = MessagesState {
            messages: vec![Message::new_human_message("Hello")],
        };
        let update = messages_state_update(vec![Message::new_ai_message("Hi!")]);

        let new_state = apply_update_to_messages_state(&state, &update);
        assert_eq!(new_state.messages.len(), 2);
    }
}
