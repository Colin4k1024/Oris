use serde::{Deserialize, Serialize};

use crate::schemas::{Message, MessageType};

/// Google Gemini model options
pub enum GeminiModel {
    /// Gemini 1.5 Flash
    Gemini15Flash,
    /// Gemini 1.5 Flash 8B
    Gemini15Flash8B,
    /// Gemini 1.5 Pro
    Gemini15Pro,
    /// Gemini 1.5 Pro Latest
    Gemini15ProLatest,
    /// Gemini 2.0 Flash
    Gemini20Flash,
    /// Gemini 2.0 Flash Thinking
    Gemini20FlashThinking,
    /// Gemini 2.5 Flash
    Gemini25Flash,
    /// Gemini 2.5 Flash Lite
    Gemini25FlashLite,
}

impl ToString for GeminiModel {
    fn to_string(&self) -> String {
        match self {
            GeminiModel::Gemini15Flash => "gemini-1.5-flash".to_string(),
            GeminiModel::Gemini15Flash8B => "gemini-1.5-flash-8b".to_string(),
            GeminiModel::Gemini15Pro => "gemini-1.5-pro".to_string(),
            GeminiModel::Gemini15ProLatest => "gemini-1.5-pro-latest".to_string(),
            GeminiModel::Gemini20Flash => "gemini-2.0-flash".to_string(),
            GeminiModel::Gemini20FlashThinking => "gemini-2.0-flash-thinking".to_string(),
            GeminiModel::Gemini25Flash => "gemini-2.5-flash".to_string(),
            GeminiModel::Gemini25FlashLite => "gemini-2.5-flash-lite".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct GeminiMessage {
    pub role: String,
    pub parts: Vec<Part>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Part {
    pub text: String,
}

impl GeminiMessage {
    pub fn new<S: Into<String>>(role: S, content: S) -> Self {
        Self {
            role: role.into(),
            parts: vec![Part {
                text: content.into(),
            }],
        }
    }

    pub fn from_message(message: &Message) -> Self {
        let role = match &message.message_type {
            MessageType::SystemMessage => "user", // Gemini doesn't have system role, use user
            MessageType::AIMessage => "model",
            MessageType::HumanMessage => "user",
            MessageType::ToolMessage => "user",
        };
        Self::new(role, &message.content)
    }
}

#[derive(Serialize, Deserialize)]
pub(crate) struct Payload {
    pub contents: Vec<GeminiMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generation_config: Option<GenerationConfig>,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct GenerationConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_k: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_sequences: Option<Vec<String>>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub(crate) struct ApiResponse {
    pub candidates: Vec<Candidate>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage_metadata: Option<UsageMetadata>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub(crate) struct Candidate {
    pub content: CandidateContent,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub(crate) struct CandidateContent {
    pub parts: Vec<Part>,
    pub role: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub(crate) struct UsageMetadata {
    pub prompt_token_count: u32,
    pub candidates_token_count: u32,
    pub total_token_count: u32,
}

// Stream response structures
#[derive(Debug, Serialize, Deserialize, Clone)]
pub(crate) struct StreamResponse {
    pub candidates: Vec<StreamCandidate>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub(crate) struct StreamCandidate {
    pub content: CandidateContent,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<String>,
}

// Error response structure
#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct ErrorResponse {
    pub error: ErrorDetail,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct ErrorDetail {
    pub code: Option<u32>,
    pub message: String,
    pub status: Option<String>,
}
