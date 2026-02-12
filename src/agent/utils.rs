//! Shared utilities for agent message handling
//!
//! This module provides common utility functions used across agent implementations,
//! particularly for converting between different message formats.

use serde_json::json;

use crate::chain::ChainError;
use crate::prompt::PromptArgs;
use crate::schemas::messages::Message;
use crate::schemas::MessageType;

/// Convert message-based input format to standard prompt args.
///
/// This function transforms input variables containing a "messages" key
/// into a standardized format with "input" and "chat_history" keys.
///
/// # Arguments
///
/// * `input_variables` - PromptArgs containing a "messages" key with Vec<Message>
///
/// # Returns
///
/// A new PromptArgs with:
/// - "input": The last human message content (or last message if no human found)
/// - "chat_history": The full message history
/// - Any other keys from the original input (except "messages")
///
/// # Errors
///
/// Returns `ChainError::OtherError` if:
/// - The "messages" key is missing
/// - The messages cannot be parsed as JSON
///
/// # Example
///
/// ```rust,ignore
/// let input = prompt_args! {
///     "messages" => vec![
///         Message::new_human_message("Hello"),
///         Message::new_ai_message("Hi there!")
///     ]
/// };
/// let result = convert_messages_to_prompt_args(input)?;
/// ```
pub fn convert_messages_to_prompt_args(input_variables: PromptArgs) -> Result<PromptArgs, ChainError> {
    let messages_value = input_variables
        .get("messages")
        .ok_or_else(|| ChainError::OtherError("Missing 'messages' key".to_string()))?;

    let messages: Vec<Message> = serde_json::from_value(messages_value.clone())
        .map_err(|e| ChainError::OtherError(format!("Failed to parse messages: {}", e)))?;

    // Extract the last user/human message as input
    let input = messages
        .iter()
        .rev()
        .find(|m| matches!(m.message_type, MessageType::HumanMessage))
        .map(|m| m.content.clone())
        .unwrap_or_else(|| {
            messages
                .last()
                .map(|m| m.content.clone())
                .unwrap_or_default()
        });

    let mut prompt_args = PromptArgs::new();
    prompt_args.insert("input".to_string(), json!(input));

    // Preserve chat history if it exists, otherwise use messages
    if input_variables.contains_key("chat_history") {
        prompt_args.insert(
            "chat_history".to_string(),
            input_variables["chat_history"].clone(),
        );
    } else {
        prompt_args.insert("chat_history".to_string(), json!(messages));
    }

    // Copy any other keys
    for (key, value) in input_variables {
        if key != "messages" && key != "chat_history" {
            prompt_args.insert(key, value);
        }
    }

    Ok(prompt_args)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schemas::Message;

    #[test]
    fn test_convert_messages_with_human_message() {
        let messages = vec![
            Message::new_system_message("You are helpful"),
            Message::new_human_message("Hello"),
            Message::new_ai_message("Hi there!"),
        ];

        let input = prompt_args! {
            "messages" => messages
        };

        let result = convert_messages_to_prompt_args(input);
        assert!(result.is_ok());
        let args = result.unwrap();
        assert_eq!(args["input"], json!("Hello"));
        assert!(args.contains_key("chat_history"));
    }

    #[test]
    fn test_convert_messages_without_human_message() {
        let messages = vec![
            Message::new_system_message("You are helpful"),
            Message::new_ai_message("I am an AI"),
        ];

        let input = prompt_args! {
            "messages" => messages
        };

        let result = convert_messages_to_prompt_args(input);
        assert!(result.is_ok());
        let args = result.unwrap();
        assert_eq!(args["input"], json!("I am an AI"));
    }

    #[test]
    fn test_convert_messages_preserves_other_keys() {
        let messages = vec![Message::new_human_message("Hello")];

        let input = prompt_args! {
            "messages" => messages,
            "custom_key" => "custom_value"
        };

        let result = convert_messages_to_prompt_args(input);
        assert!(result.is_ok());
        let args = result.unwrap();
        assert_eq!(args["custom_key"], json!("custom_value"));
    }

    #[test]
    fn test_convert_messages_missing_key() {
        let input = prompt_args! {
            "other_key" => "value"
        };

        let result = convert_messages_to_prompt_args(input);
        assert!(result.is_err());
    }
}
