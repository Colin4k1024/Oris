use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub(crate) struct Payload {
    pub inputs: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parameters: Option<GenerationParameters>,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct GenerationParameters {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_new_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_k: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub do_sample: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop: Option<Vec<String>>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub(crate) struct ApiResponse {
    pub generated_text: String,
}

// For streaming responses
#[derive(Debug, Serialize, Deserialize, Clone)]
pub(crate) struct StreamResponse {
    pub token: StreamToken,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub(crate) struct StreamToken {
    pub text: String,
    pub id: u32,
    pub special: bool,
}

// Error response structure
#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct ErrorResponse {
    pub error: String,
}
