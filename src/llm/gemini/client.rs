use crate::{
    language_models::{llm::LLM, options::CallOptions, GenerateResult, LLMError, TokenUsage},
    llm::GeminiError,
    schemas::{Message, StreamData},
};
use async_trait::async_trait;
use futures::{Stream, StreamExt};
use reqwest::Client;
use serde_json::Value;
use std::{pin::Pin, str, str::from_utf8};

use super::models::{ApiResponse, ErrorResponse, GeminiMessage, GenerationConfig, Payload};

/// Parse error from JSON response and return appropriate GeminiError
fn parse_error_response(status: u16, message: &str) -> LLMError {
    match status {
        400 => LLMError::GeminiError(GeminiError::InvalidParameterError(message.to_string())),
        401 => LLMError::GeminiError(GeminiError::InvalidApiKeyError(message.to_string())),
        403 => LLMError::GeminiError(GeminiError::PermissionError(message.to_string())),
        429 => LLMError::GeminiError(GeminiError::RateLimitError(message.to_string())),
        500 => LLMError::GeminiError(GeminiError::InternalError(message.to_string())),
        503 => LLMError::GeminiError(GeminiError::ModelUnavailableError(message.to_string())),
        529 => LLMError::GeminiError(GeminiError::ResourceExhaustedError(message.to_string())),
        _ => LLMError::GeminiError(GeminiError::SystemError(message.to_string())),
    }
}

/// Google Gemini client
#[derive(Clone)]
pub struct Gemini {
    model: String,
    options: CallOptions,
    api_key: String,
    base_url: String,
}

impl Default for Gemini {
    fn default() -> Self {
        Self::new()
    }
}

impl Gemini {
    /// Create a new Gemini client with default settings
    pub fn new() -> Self {
        Self {
            model: "gemini-1.5-flash".to_string(),
            options: CallOptions::default(),
            api_key: std::env::var("GOOGLE_API_KEY").unwrap_or_default(),
            base_url: "https://generativelanguage.googleapis.com/v1beta".to_string(),
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
        self.api_key = api_key.into();
        self
    }

    /// Set the base URL
    pub fn with_base_url<S: Into<String>>(mut self, base_url: S) -> Self {
        self.base_url = base_url.into();
        self
    }

    /// Generates text using the Gemini API
    async fn generate(&self, messages: &[Message]) -> Result<GenerateResult, LLMError> {
        let client = Client::new();
        let is_stream = self.options.streaming_func.is_some();

        let payload = self.build_payload(messages, is_stream);
        let url = format!("{}/models/{}:generateContent", self.base_url, self.model);
        let res = client
            .post(&url)
            .query(&[("key", &self.api_key)])
            .header("Content-Type", "application/json")
            .json(&payload)
            .send()
            .await?;

        match res.status().as_u16() {
            200 => {
                let api_response = res.json::<ApiResponse>().await?;

                // Extract the first candidate content
                let generation = match api_response.candidates.first() {
                    Some(candidate) => match candidate.content.parts.first() {
                        Some(part) => part.text.clone(),
                        None => {
                            return Err(LLMError::ContentNotFound(
                                "No content in candidate".to_string(),
                            ))
                        }
                    },
                    None => {
                        return Err(LLMError::ContentNotFound(
                            "No candidates returned from API".to_string(),
                        ))
                    }
                };

                let tokens = api_response.usage_metadata.map(|usage| TokenUsage {
                    prompt_tokens: usage.prompt_token_count,
                    completion_tokens: usage.candidates_token_count,
                    total_tokens: usage.total_token_count,
                });

                Ok(GenerateResult { tokens, generation })
            }
            status => {
                let error_message = res
                    .json::<ErrorResponse>()
                    .await
                    .map(|e| e.error.message)
                    .unwrap_or_else(|_| format!("HTTP {}", status));
                Err(parse_error_response(status, &error_message))
            }
        }
    }

    /// Builds the API payload from messages
    fn build_payload(&self, messages: &[Message], _delete_collection: bool) -> Payload {
        // Gemini requires system message to be first user message
        let gemini_messages: Vec<GeminiMessage> =
            messages.iter().map(GeminiMessage::from_message).collect();

        // If first message is system, convert it to user message
        // (Already handled in from_message conversion)

        let generation_config = GenerationConfig {
            temperature: self.options.temperature,
            max_output_tokens: self.options.max_tokens,
            top_p: self.options.top_p,
            top_k: self.options.top_k.map(|k| k as u32),
            stop_sequences: self.options.stop_words.clone(),
        };

        // Remove None values
        if generation_config.temperature.is_none()
            && generation_config.max_output_tokens.is_none()
            && generation_config.top_p.is_none()
            && generation_config.top_k.is_none()
            && generation_config.stop_sequences.is_none()
        {
            Payload {
                contents: gemini_messages,
                generation_config: None,
            }
        } else {
            Payload {
                contents: gemini_messages,
                generation_config: Some(generation_config),
            }
        }
    }

    /// Parse Server-Sent Events (SSE) chunks
    fn parse_sse_chunk(bytes: &[u8]) -> Result<Vec<Value>, LLMError> {
        let text = from_utf8(bytes).map_err(|e| LLMError::OtherError(e.to_string()))?;
        let mut values = Vec::new();

        for line in text.lines() {
            if line.starts_with("data: ") {
                let data = &line[6..];
                if data == "[DONE]" {
                    continue;
                }

                match serde_json::from_str::<Value>(data) {
                    Ok(value) => values.push(value),
                    Err(e) => {
                        return Err(LLMError::OtherError(format!(
                            "Failed to parse SSE data: {}, data: {}",
                            e, data
                        )));
                    }
                }
            }
        }

        Ok(values)
    }
}

#[async_trait]
impl LLM for Gemini {
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
        let payload = self.build_payload(messages, true);
        let url = format!(
            "{}/models/{}:streamGenerateContent",
            self.base_url, self.model
        );
        let request = client
            .post(&url)
            .query(&[("key", &self.api_key)])
            .header("Content-Type", "application/json")
            .json(&payload)
            .build()?;

        let stream = client.execute(request).await?;
        let stream = stream.bytes_stream();

        let processed_stream = stream
            .then(move |result| {
                async move {
                    match result {
                        Ok(bytes) => {
                            // Parse SSE chunk format
                            let _bytes_str = from_utf8(&bytes)
                                .map_err(|e| LLMError::OtherError(e.to_string()))?;
                            let chunks = Gemini::parse_sse_chunk(&bytes)?;

                            for chunk in chunks {
                                if let Some(candidates) =
                                    chunk.get("candidates").and_then(|c| c.as_array())
                                {
                                    if let Some(candidate) = candidates.first() {
                                        if let Some(content) = candidate.get("content") {
                                            if let Some(parts) =
                                                content.get("parts").and_then(|p| p.as_array())
                                            {
                                                if let Some(part) = parts.first() {
                                                    if let Some(text) =
                                                        part.get("text").and_then(|t| t.as_str())
                                                    {
                                                        if !text.is_empty() {
                                                            let usage = chunk
                                                                .get("usageMetadata")
                                                                .map(|usage| TokenUsage {
                                                                    prompt_tokens: usage
                                                                        .get("promptTokenCount")
                                                                        .and_then(|t| t.as_u64())
                                                                        .unwrap_or(0)
                                                                        as u32,
                                                                    completion_tokens: usage
                                                                        .get("candidatesTokenCount")
                                                                        .and_then(|t| t.as_u64())
                                                                        .unwrap_or(0)
                                                                        as u32,
                                                                    total_tokens: usage
                                                                        .get("totalTokenCount")
                                                                        .and_then(|t| t.as_u64())
                                                                        .unwrap_or(0)
                                                                        as u32,
                                                                });

                                                            return Ok(StreamData::new(
                                                                chunk.clone(),
                                                                usage,
                                                                text,
                                                            ));
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }

                            // If we didn't return within the loop, return an empty stream data
                            Ok(StreamData::new(Value::Null, None, ""))
                        }
                        Err(e) => Err(LLMError::RequestError(e)),
                    }
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
    use tokio::test;

    #[test]
    #[ignore]
    async fn test_gemini_generate() {
        let gemini = Gemini::new();

        let res = gemini
            .generate(&[Message::new_human_message("Hello!")])
            .await
            .unwrap();

        println!("{:?}", res)
    }

    #[test]
    #[ignore]
    async fn test_gemini_stream() {
        let gemini = Gemini::new();
        let mut stream = gemini
            .stream(&[Message::new_human_message("Hello!")])
            .await
            .unwrap();
        while let Some(data) = stream.next().await {
            match data {
                Ok(value) => value.to_stdout().unwrap(),
                Err(e) => panic!("Error invoking Gemini: {:?}", e),
            }
        }
    }
}
