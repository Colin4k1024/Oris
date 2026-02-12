//! Integration tests for the agent module
//!
//! These tests verify the behavior of agent components including
//! message conversion, agent creation, and error handling.
//!
//! ## Running Integration Tests
//!
//! Some tests require external API keys and are marked with `#[ignore]`.
//! Run them with:
//! ```bash
//! cargo test --features openai,ollama -- --ignored
//! ```
//!
//! ## Required Environment Variables
//!
//! - `OPENAI_API_KEY`: OpenAI API key for OpenAI provider tests
//! - `ANTHROPIC_API_KEY`: Anthropic API key for Claude tests
//! - `OLLAMA_HOST`: Ollama host URL (default: http://localhost:11434)

use crate::agent::utils::convert_messages_to_prompt_args;
use crate::prompt_args;
use crate::schemas::Message;
use crate::schemas::MessageType;
use crate::ChainError;

/// Unit tests for convert_messages_to_prompt_args function
///
/// These tests verify the message-to-prompt-args conversion logic
/// without requiring external API calls.

#[cfg(test)]
mod unit_tests {
    use super::*;

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
        assert!(result.is_ok(), "Conversion should succeed");
        let args = result.unwrap();
        assert_eq!(args["input"], serde_json::json!("Hello"));
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
        // Should fall back to last message
        assert_eq!(args["input"], serde_json::json!("I am an AI"));
    }

    #[test]
    fn test_convert_messages_preserves_custom_keys() {
        let messages = vec![Message::new_human_message("Hello")];

        let input = prompt_args! {
            "messages" => messages,
            "custom_key" => "custom_value",
            "another_key" => 42
        };

        let result = convert_messages_to_prompt_args(input);
        assert!(result.is_ok());
        let args = result.unwrap();
        assert_eq!(args["custom_key"], serde_json::json!("custom_value"));
        assert_eq!(args["another_key"], serde_json::json!(42));
    }

    #[test]
    fn test_convert_messages_preserves_chat_history() {
        let messages = vec![
            Message::new_human_message("First"),
            Message::new_ai_message("Response"),
        ];

        let input = prompt_args! {
            "messages" => messages,
            "chat_history" => serde_json::json!(vec![
                Message::new_human_message("Previous")
            ])
        };

        let result = convert_messages_to_prompt_args(input);
        assert!(result.is_ok());
        let args = result.unwrap();
        
        // Should preserve existing chat_history, not overwrite with messages
        let chat_history = args["chat_history"]
            .as_array()
            .expect("chat_history should be an array");
        assert_eq!(chat_history.len(), 1);
        assert_eq!(
            chat_history[0]["content"],
            serde_json::json!("Previous")
        );
    }

    #[test]
    fn test_convert_messages_missing_messages_key() {
        let input = prompt_args! {
            "other_key" => "value"
        };

        let result = convert_messages_to_prompt_args(input);
        assert!(result.is_err(), "Should error on missing 'messages' key");
    }

    #[test]
    fn test_convert_messages_invalid_json() {
        let input = prompt_args! {
            "messages" => "not a valid json array"
        };

        let result = convert_messages_to_prompt_args(input);
        assert!(result.is_err(), "Should error on invalid JSON");
    }

    #[test]
    fn test_convert_empty_messages() {
        let messages: Vec<Message> = vec![];
        let input = prompt_args! {
            "messages" => messages
        };

        let result = convert_messages_to_prompt_args(input);
        assert!(result.is_ok());
        let args = result.unwrap();
        // Empty messages, should default to empty string
        assert_eq!(args["input"], serde_json::json!(""));
    }

    #[test]
    fn test_convert_messages_multiple_human_takes_last() {
        let messages = vec![
            Message::new_human_message("First human"),
            Message::new_ai_message("AI response"),
            Message::new_human_message("Second human"),
        ];

        let input = prompt_args! {
            "messages" => messages
        };

        let result = convert_messages_to_prompt_args(input);
        assert!(result.is_ok());
        let args = result.unwrap();
        // Should take the last human message
        assert_eq!(args["input"], serde_json::json!("Second human"));
    }
}

/// Integration tests requiring external services
///
/// These tests are marked with `#[ignore]` and require:
/// - Valid API keys set in environment
/// - Running external services (e.g., Ollama locally)
#[cfg(test)]
mod integration_tests {
    use crate::agent::{create_agent, ConversationalAgentBuilder};
    use crate::llm::openai::{OpenAI, OpenAIModel};

    #[tokio::test]
    #[ignore = "Requires OPENAI_API_KEY environment variable"]
    async fn test_agent_with_openai() -> Result<(), Box<dyn std::error::Error>> {
        let llm = OpenAI::new(
            crate::llm::openai::OpenAIConfig::new()
                .with_api_key(std::env::var("OPENAI_API_KEY")?),
        )
        .with_model(OpenAIModel::Gpt35);

        let agent = ConversationalAgentBuilder::new()
            .llm(llm)
            .prefix("You are a helpful assistant.".to_string())
            .build()?;

        let result = agent
            .invoke("Hello, how are you?".to_string())
            .await?;
        
        assert!(!result.is_empty(), "Response should not be empty");
        Ok(())
    }

    #[tokio::test]
    #[ignore = "Requires running Ollama instance (ollama serve)"]
    async fn test_agent_with_ollama() -> Result<(), Box<dyn std::error::Error>> {
        let llm = crate::llm::ollama::Ollama::new(
            "llama3".to_string(),
            Some(std::env::var("OLLAMA_HOST").unwrap_or_else(|_| "http://localhost:11434".to_string())),
        );

        let agent = ConversationalAgentBuilder::new()
            .llm(llm)
            .prefix("You are a helpful assistant.".to_string())
            .build()?;

        let result = agent
            .invoke("Hello, how are you?".to_string())
            .await?;
        
        assert!(!result.is_empty(), "Response should not be empty");
        Ok(())
    }
}

/// End-to-end workflow tests
///
/// These tests verify complete workflows including:
/// - Agent creation and invocation
/// - Message handling
/// - Error propagation
#[cfg(test)]
mod workflow_tests {
    use crate::chain::Chain;
    use crate::llm::openai::{OpenAI, OpenAIModel};
    use crate::prompt::HumanMessagePromptTemplate;
    use crate::message_formatter;
    use crate::prompt_args;
    use crate::template_fstring;

    /// Test that demonstrates the expected workflow for creating
    /// and invoking a basic LLM chain.
    ///
    /// This test can be used as a reference for users integrating
    /// the library into their projects.
    #[tokio::test]
    #[ignore = "Requires OPENAI_API_KEY - run with: cargo test --features openai test_basic_workflow -- --ignored"]
    async fn test_basic_workflow() {
        // Step 1: Create a prompt template
        let prompt = HumanMessagePromptTemplate::new(template_fstring!(
            "Translate this to French: {text}",
            "text",
        ));
        let formatter = message_formatter![prompt.into()];

        // Step 2: Configure the LLM
        let llm = OpenAI::default()
            .with_model(OpenAIModel::Gpt35.to_string());

        // Step 3: Build the chain
        let chain = crate::chain::LLMChainBuilder::new()
            .prompt(formatter)
            .llm(llm)
            .build()
            .expect("Failed to build chain");

        // Step 4: Invoke the chain
        let result = chain
            .invoke(prompt_args! { "text" => "Hello world" })
            .await;

        assert!(result.is_ok(), "Chain should execute successfully");
        let response = result.unwrap();
        assert!(response.contains("Bonjour") || response.contains("French"),
            "Response should contain French translation");
    }
}
