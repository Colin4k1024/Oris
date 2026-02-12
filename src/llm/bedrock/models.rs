use serde::{Deserialize, Serialize};

use crate::schemas::{Message, MessageType};

/// AWS Bedrock model options
pub enum BedrockModel {
    /// Claude 3.5 Sonnet
    Claude35Sonnet,
    /// Claude 3 Opus
    Claude3Opus,
    /// Claude 3 Sonnet
    Claude3Sonnet,
    /// Claude 3 Haiku
    Claude3Haiku,
    /// Llama 2 70B Chat
    Llama270BChat,
    /// Llama 3 70B Instruct
    Llama370BInstruct,
    /// Llama 3 8B Instruct
    Llama38BInstruct,
    /// Titan Text G1 Large
    TitanTextG1Large,
    /// Titan Text G1 Express
    TitanTextG1Express,
}

impl ToString for BedrockModel {
    fn to_string(&self) -> String {
        match self {
            BedrockModel::Claude35Sonnet => "anthropic.claude-3-5-sonnet-20240620-v1:0".to_string(),
            BedrockModel::Claude3Opus => "anthropic.claude-3-opus-20240229-v1:0".to_string(),
            BedrockModel::Claude3Sonnet => "anthropic.claude-3-sonnet-20240229-v1:0".to_string(),
            BedrockModel::Claude3Haiku => "anthropic.claude-3-haiku-20240307-v1:0".to_string(),
            BedrockModel::Llama270BChat => "meta.llama2-70b-chat-v1".to_string(),
            BedrockModel::Llama370BInstruct => "meta.llama3-70b-instruct-v1:0".to_string(),
            BedrockModel::Llama38BInstruct => "meta.llama3-8b-instruct-v1:0".to_string(),
            BedrockModel::TitanTextG1Large => "amazon.titan-text-lite-v1".to_string(),
            BedrockModel::TitanTextG1Express => "amazon.titan-text-express-v1".to_string(),
        }
    }
}

#[derive(Serialize, Deserialize)]
pub(crate) struct BedrockMessage {
    pub role: String,
    pub content: String,
}

impl BedrockMessage {
    pub fn new<S: Into<String>>(role: S, content: S) -> Self {
        Self {
            role: role.into(),
            content: content.into(),
        }
    }

    pub fn from_message(message: &Message) -> Self {
        match &message.message_type {
            MessageType::SystemMessage => Self::new("user", &message.content),
            MessageType::AIMessage => Self::new("assistant", &message.content),
            MessageType::HumanMessage => Self::new("user", &message.content),
            MessageType::ToolMessage => Self::new("user", &message.content),
        }
    }
}

#[derive(Serialize, Deserialize)]
pub(crate) struct Payload {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub anthropic_version: Option<String>,
    pub messages: Vec<BedrockMessage>,
    #[serde(rename = "max_tokens")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_k: Option<u32>,
    #[serde(rename = "stop_sequences")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_sequences: Option<Vec<String>>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub(crate) struct ApiResponse {
    #[serde(alias = "content")]
    pub content: Vec<ContentBlock>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
    // Anthropic Claude format also supports "id" and "model" fields
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub(crate) struct ContentBlock {
    pub text: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub(crate) struct Usage {
    pub input_tokens: u32,
    pub output_tokens: u32,
}

// Error response structure
#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct ErrorResponse {
    pub message: String,
    #[serde(rename = "type")]
    pub error_type: Option<String>,
}
