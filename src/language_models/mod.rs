use std::collections::HashMap;

use serde::{Deserialize, Serialize};

pub mod common_config;
pub mod configurable;
pub mod init;
pub mod invocation_config;
pub mod llm;
pub mod model_parser;
pub mod options;
pub mod usage;

mod error;
pub use error::*;

pub use common_config::{LLMBuilder, LLMConfig, LLMHelpers, LLMInitConfig, StreamingLLM};
pub use configurable::ConfigurableModel;
pub use init::init_chat_model;
pub use invocation_config::InvocationConfig;
pub use model_parser::{parse_model_string, ParsedModel};
pub use usage::{CollectingUsageCallback, UsageCallback, UsageMetadata};

// Note: Consider adding a `data: Option<serde::Value>` field to store additional
// metadata from LLM responses (e.g., OpenAI function call responses, tool calls, etc.)
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct GenerateResult {
    pub tokens: Option<TokenUsage>,
    pub generation: String,
}

impl GenerateResult {
    pub fn to_hashmap(&self) -> HashMap<String, String> {
        let mut map = HashMap::new();

        // Insert the 'generation' field into the hashmap
        map.insert("generation".to_string(), self.generation.clone());

        // Check if 'tokens' is Some and insert its fields into the hashmap
        if let Some(ref tokens) = self.tokens {
            map.insert(
                "prompt_tokens".to_string(),
                tokens.prompt_tokens.to_string(),
            );
            map.insert(
                "completion_tokens".to_string(),
                tokens.completion_tokens.to_string(),
            );
            map.insert("total_tokens".to_string(), tokens.total_tokens.to_string());
        }

        map
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct TokenUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

impl TokenUsage {
    pub fn sum(&self, other: &TokenUsage) -> TokenUsage {
        TokenUsage {
            prompt_tokens: self.prompt_tokens + other.prompt_tokens,
            completion_tokens: self.completion_tokens + other.completion_tokens,
            total_tokens: self.total_tokens + other.total_tokens,
        }
    }

    pub fn add(&mut self, other: &TokenUsage) {
        self.prompt_tokens += other.prompt_tokens;
        self.completion_tokens += other.completion_tokens;
        self.total_tokens += other.total_tokens;
    }
}

impl TokenUsage {
    pub fn new(prompt_tokens: u32, completion_tokens: u32) -> Self {
        Self {
            prompt_tokens,
            completion_tokens,
            total_tokens: prompt_tokens + completion_tokens,
        }
    }
}
