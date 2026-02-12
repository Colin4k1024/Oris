use serde::{Deserialize, Serialize};

use crate::schemas::{Message, MessageType};

/// MistralAI model options
pub enum MistralAIModel {
    /// Mistral Small (22B)
    MistralSmall,
    /// Mistral Medium (latest)
    MistralMedium,
    /// Mistral Large (latest)
    MistralLarge,
    /// Mistral Large 2402
    MistralLarge2402,
    /// Mistral Large 2407
    MistralLarge2407,
    /// Mistral Small 2402
    MistralSmall2402,
    /// Mistral Small 2409
    MistralSmall2409,
    /// Pixtral Large
    PixtralLarge,
    /// Pixtral Large 2409
    PixtralLarge2409,
    /// Mistral 7B Instruct
    Mistral7BInstruct,
    /// Mixtral 8x7B Instruct
    Mixtral8x7BInstruct,
    /// Mixtral 8x22B Instruct
    Mixtral8x22BInstruct,
}

impl ToString for MistralAIModel {
    fn to_string(&self) -> String {
        match self {
            MistralAIModel::MistralSmall => "mistral-small-latest".to_string(),
            MistralAIModel::MistralMedium => "mistral-medium-latest".to_string(),
            MistralAIModel::MistralLarge => "mistral-large-latest".to_string(),
            MistralAIModel::MistralLarge2402 => "mistral-large-2402".to_string(),
            MistralAIModel::MistralLarge2407 => "mistral-large-2407".to_string(),
            MistralAIModel::MistralSmall2402 => "mistral-small-2402".to_string(),
            MistralAIModel::MistralSmall2409 => "mistral-small-2409".to_string(),
            MistralAIModel::PixtralLarge => "pixtral-large-latest".to_string(),
            MistralAIModel::PixtralLarge2409 => "pixtral-large-2409".to_string(),
            MistralAIModel::Mistral7BInstruct => "mistral-7b-instruct".to_string(),
            MistralAIModel::Mixtral8x7BInstruct => "mixtral-8x7b-instruct".to_string(),
            MistralAIModel::Mixtral8x22BInstruct => "mixtral-8x22b-instruct".to_string(),
        }
    }
}

#[derive(Serialize, Deserialize)]
pub(crate) struct MistralAIMessage {
    pub role: String,
    pub content: String,
}

impl MistralAIMessage {
    pub fn new<S: Into<String>>(role: S, content: S) -> Self {
        Self {
            role: role.into(),
            content: content.into(),
        }
    }

    pub fn from_message(message: &Message) -> Self {
        match message.message_type {
            MessageType::SystemMessage => Self::new("system", &message.content),
            MessageType::AIMessage => Self::new("assistant", &message.content),
            MessageType::HumanMessage => Self::new("user", &message.content),
            MessageType::ToolMessage => Self::new("user", &message.content),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub(crate) struct ApiResponse {
    pub id: String,
    pub created: u64,
    pub model: String,
    pub choices: Vec<Choice>,
    pub usage: Usage,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub(crate) struct Choice {
    pub message: ResponseMessage,
    pub finish_reason: Option<String>,
    pub index: u32,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub(crate) struct ResponseMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub(crate) struct Usage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

// Stream response structures
#[derive(Debug, Serialize, Deserialize, Clone)]
pub(crate) struct StreamResponse {
    pub id: String,
    pub model: String,
    pub created: u64,
    pub choices: Vec<StreamChoice>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub(crate) struct StreamChoice {
    pub delta: Delta,
    pub finish_reason: Option<String>,
    pub index: u32,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub(crate) struct Delta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct Payload {
    pub model: String,
    pub messages: Vec<MistralAIMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "random_seed")]
    pub random_seed: Option<u64>,
}

// Error response structure
#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct ErrorResponse {
    pub code: Option<String>,
    pub message: String,
    #[serde(rename = "type")]
    pub error_type: Option<String>,
}
