use crate::{
    language_models::{llm::LLM, options::CallOptions, GenerateResult, LLMError},
    llm::HuggingFaceError,
    schemas::{Message, StreamData},
};
use async_trait::async_trait;
use futures::{Stream, StreamExt};
use reqwest::Client;
use serde_json::Value;
use std::pin::Pin;

use super::models::{ApiResponse, GenerationParameters, Payload};

/// Parse error from response and return appropriate HuggingFaceError
fn parse_error_response(status: u16, message: &str) -> LLMError {
    match status {
        400 => {
            LLMError::HuggingFaceError(HuggingFaceError::InvalidParameterError(message.to_string()))
        }
        401 => {
            LLMError::HuggingFaceError(HuggingFaceError::InvalidApiKeyError(message.to_string()))
        }
        404 => {
            LLMError::HuggingFaceError(HuggingFaceError::ModelNotFoundError(message.to_string()))
        }
        429 => LLMError::HuggingFaceError(HuggingFaceError::RateLimitError(message.to_string())),
        500 => LLMError::HuggingFaceError(HuggingFaceError::InternalError(message.to_string())),
        503 => {
            LLMError::HuggingFaceError(HuggingFaceError::ModelUnavailableError(message.to_string()))
        }
        _ => LLMError::HuggingFaceError(HuggingFaceError::SystemError(message.to_string())),
    }
}

/// HuggingFace client
#[derive(Clone)]
pub struct HuggingFace {
    model: String,
    options: CallOptions,
    api_key: Option<String>,
    base_url: String,
}

impl Default for HuggingFace {
    fn default() -> Self {
        Self::new()
    }
}

impl HuggingFace {
    /// Create a new HuggingFace client with default settings
    pub fn new() -> Self {
        Self {
            model: "microsoft/Phi-3-mini-4k-instruct".to_string(),
            options: CallOptions::default(),
            api_key: std::env::var("HUGGINGFACE_API_KEY").ok(),
            base_url: "https://api-inference.huggingface.co/models".to_string(),
        }
    }

    /// Set the model
    pub fn with_model<S: Into<String>>(mut self, model: S) -> Self {
        self.model = model.into();
        self
    }

    /// Set call options
    pub fn with_options(mut self, options: CallOptions) -> Self {
        self.options = options;
        self
    }

    /// Set API key
    pub fn with_api_key<S: Into<String>>(mut self, api_key: S) -> Self {
        self.api_key = Some(api_key.into());
        self
    }

    /// Set the base URL
    pub fn with_base_url<S: Into<String>>(mut self, base_url: S) -> Self {
        self.base_url = base_url.into();
        self
    }

    /// Generates text using the HuggingFace API
    async fn generate(&self, messages: &[Message]) -> Result<GenerateResult, LLMError> {
        let client = Client::new();
        let payload = self.build_payload(messages);

        let url = format!("{}/{}", self.base_url, self.model);
        let mut request = client.post(&url).json(&payload);

        if let Some(key) = &self.api_key {
            request = request.header("Authorization", format!("Bearer {}", key));
        }

        let res = request.send().await?;

        match res.status().as_u16() {
            200 => {
                let api_response: Vec<ApiResponse> = res.json().await?;
                let generation = api_response
                    .first()
                    .map(|r| r.generated_text.clone())
                    .unwrap_or_default();

                Ok(GenerateResult {
                    tokens: None, // HuggingFace API doesn't always return token usage
                    generation,
                })
            }
            status => {
                let error_message = res
                    .text()
                    .await
                    .unwrap_or_else(|_| format!("HTTP {}", status));
                Err(parse_error_response(status, &error_message))
            }
        }
    }

    /// Builds the API payload from messages
    fn build_payload(&self, messages: &[Message]) -> Payload {
        // Combine all messages into a single input string
        let input = messages
            .iter()
            .map(|m| {
                let prefix = match &m.message_type {
                    crate::schemas::MessageType::SystemMessage => "System: ",
                    crate::schemas::MessageType::AIMessage => "Assistant: ",
                    crate::schemas::MessageType::HumanMessage => "User: ",
                    crate::schemas::MessageType::ToolMessage => "Tool: ",
                };
                format!("{}{}", prefix, m.content)
            })
            .collect::<Vec<_>>()
            .join("\n");

        let parameters = GenerationParameters {
            max_new_tokens: self.options.max_tokens,
            temperature: self.options.temperature,
            top_p: self.options.top_p,
            top_k: self.options.top_k.map(|k| k as u32),
            do_sample: if self.options.temperature.is_some() {
                Some(true)
            } else {
                None
            },
            stop: self.options.stop_words.clone(),
        };

        // Remove None values
        if parameters.max_new_tokens.is_none()
            && parameters.temperature.is_none()
            && parameters.top_p.is_none()
            && parameters.top_k.is_none()
            && parameters.stop.is_none()
        {
            Payload {
                inputs: input,
                parameters: None,
            }
        } else {
            Payload {
                inputs: input,
                parameters: Some(parameters),
            }
        }
    }
}

#[async_trait]
impl LLM for HuggingFace {
    async fn generate(&self, messages: &[Message]) -> Result<GenerateResult, LLMError> {
        match &self.options.streaming_func {
            Some(func) => {
                let mut complete_response = String::new();
                let mut stream = self.stream(messages).await?;
                while let Some(data) = stream.next().await {
                    match data {
                        Ok(value) => {
                            let mut func = func.lock().await;
                            complete_response.push_str(&value.content);
                            let _ = func(value.content).await;
                        }
                        Err(e) => return Err(e),
                    }
                }
                let mut generate_result = GenerateResult::default();
                generate_result.generation = complete_response;
                Ok(generate_result)
            }
            None => self.generate(messages).await,
        }
    }

    async fn stream(
        &self,
        messages: &[Message],
    ) -> Result<Pin<Box<dyn Stream<Item = Result<StreamData, LLMError>> + Send>>, LLMError> {
        let client = Client::new();
        let payload = self.build_payload(messages);

        let url = format!("{}/{}", self.base_url, self.model);
        let mut request = client
            .post(&url)
            .json(&payload)
            .header("Accept", "text/event-stream");

        if let Some(key) = &self.api_key {
            request = request.header("Authorization", format!("Bearer {}", key));
        }

        let stream = request.send().await?.bytes_stream();

        let processed_stream = stream
            .then(move |result| async move {
                match result {
                    Ok(bytes) => {
                        let text = String::from_utf8_lossy(&bytes);
                        for line in text.lines() {
                            if line.starts_with("data: ") {
                                let data = &line[6..];
                                if data == "[DONE]" {
                                    continue;
                                }
                                if let Ok(value) = serde_json::from_str::<Value>(data) {
                                    if let Some(text) = value
                                        .get("token")
                                        .and_then(|t| t.get("text"))
                                        .and_then(|t| t.as_str())
                                    {
                                        if !text.is_empty() {
                                            return Ok(StreamData::new(value.clone(), None, text));
                                        }
                                    }
                                }
                            }
                        }
                        Ok(StreamData::new(Value::Null, None, ""))
                    }
                    Err(e) => Err(LLMError::RequestError(e)),
                }
            })
            .filter_map(|result| async move {
                match result {
                    Ok(data) if !data.content.is_empty() => Some(Ok(data)),
                    Ok(_) => None,
                    Err(e) => Some(Err(e)),
                }
            });

        Ok(Box::pin(processed_stream))
    }

    fn add_options(&mut self, options: CallOptions) {
        self.options.merge_options(options)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore]
    async fn test_huggingface_generate() {
        let hf = HuggingFace::new();

        let res = hf
            .generate(&[Message::new_human_message("Hello!")])
            .await
            .unwrap();

        println!("{:?}", res)
    }
}
