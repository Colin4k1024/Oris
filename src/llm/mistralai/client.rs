use crate::{
    language_models::{llm::LLM, options::CallOptions, GenerateResult, LLMError, TokenUsage},
    llm::MistralAIError,
    schemas::{Message, StreamData},
};
use async_trait::async_trait;
use futures::{Stream, StreamExt};
use reqwest::Client;
use serde_json::Value;
use std::{pin::Pin, str, str::from_utf8};

use super::models::{ApiResponse, ErrorResponse, MistralAIMessage, Payload};

/// Parse error from JSON response and return appropriate MistralAIError
fn parse_error_response(status: u16, message: &str) -> LLMError {
    match status {
        400 => LLMError::MistralAIError(MistralAIError::InvalidParameterError(message.to_string())),
        401 => LLMError::MistralAIError(MistralAIError::InvalidApiKeyError(message.to_string())),
        429 => LLMError::MistralAIError(MistralAIError::RateLimitError(message.to_string())),
        500 => LLMError::MistralAIError(MistralAIError::InternalError(message.to_string())),
        503 => LLMError::MistralAIError(MistralAIError::ModelUnavailableError(message.to_string())),
        _ => LLMError::MistralAIError(MistralAIError::SystemError(message.to_string())),
    }
}

/// MistralAI client
#[derive(Clone)]
pub struct MistralAI {
    model: String,
    options: CallOptions,
    api_key: String,
    base_url: String,
}

impl Default for MistralAI {
    fn default() -> Self {
        Self::new()
    }
}

impl MistralAI {
    /// Create a new MistralAI client with default settings
    pub fn new() -> Self {
        Self {
            model: "mistral-small-latest".to_string(),
            options: CallOptions::default(),
            api_key: std::env::var("MISTRAL_API_KEY").unwrap_or_default(),
            base_url: "https://api.mistral.ai/v1/chat/completions".to_string(),
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

    /// Generates text using the MistralAI API
    async fn generate(&self, messages: &[Message]) -> Result<GenerateResult, LLMError> {
        let client = Client::new();
        let is_stream = self.options.streaming_func.is_some();

        let payload = self.build_payload(messages, is_stream);
        let res = client
            .post(&self.base_url)
            .header("Authorization", format!("Bearer {}", &self.api_key))
            .header("Content-Type", "application/json")
            .json(&payload)
            .send()
            .await?;

        match res.status().as_u16() {
            200 => {
                let api_response = res.json::<ApiResponse>().await?;

                // Extract the first choice content
                let generation = match api_response.choices.first() {
                    Some(choice) => choice.message.content.clone(),
                    None => {
                        return Err(LLMError::ContentNotFound(
                            "No content returned from API".to_string(),
                        ))
                    }
                };

                let tokens = Some(TokenUsage {
                    prompt_tokens: api_response.usage.prompt_tokens,
                    completion_tokens: api_response.usage.completion_tokens,
                    total_tokens: api_response.usage.total_tokens,
                });

                Ok(GenerateResult { tokens, generation })
            }
            status => {
                let error_message = res
                    .json::<ErrorResponse>()
                    .await
                    .map(|e| e.message)
                    .unwrap_or_else(|_| format!("HTTP {}", status));
                Err(parse_error_response(status, &error_message))
            }
        }
    }

    /// Builds the API payload from messages
    fn build_payload(&self, messages: &[Message], stream: bool) -> Payload {
        let mut payload = Payload {
            model: self.model.clone(),
            messages: messages
                .iter()
                .map(MistralAIMessage::from_message)
                .collect::<Vec<_>>(),
            max_tokens: self.options.max_tokens,
            stream: None,
            stop: self.options.stop_words.clone(),
            temperature: self.options.temperature,
            top_p: self.options.top_p,
            random_seed: None,
        };

        if stream {
            payload.stream = Some(true);
        }

        payload
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
impl LLM for MistralAI {
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
        let request = client
            .post(&self.base_url)
            .header("Authorization", format!("Bearer {}", &self.api_key))
            .header("Content-Type", "application/json")
            .header("Accept", "text/event-stream")
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
                            let chunks = MistralAI::parse_sse_chunk(&bytes)?;

                            for chunk in chunks {
                                if let Some(choices) =
                                    chunk.get("choices").and_then(|c| c.as_array())
                                {
                                    if let Some(choice) = choices.first() {
                                        if let Some(delta) = choice.get("delta") {
                                            // Extract content from delta
                                            if let Some(content) =
                                                delta.get("content").and_then(|c| c.as_str())
                                            {
                                                if !content.is_empty() {
                                                    let usage =
                                                        if let Some(usage) = chunk.get("usage") {
                                                            Some(TokenUsage {
                                                                prompt_tokens: usage
                                                                    .get("prompt_tokens")
                                                                    .and_then(|t| t.as_u64())
                                                                    .unwrap_or(0)
                                                                    as u32,
                                                                completion_tokens: usage
                                                                    .get("completion_tokens")
                                                                    .and_then(|t| t.as_u64())
                                                                    .unwrap_or(0)
                                                                    as u32,
                                                                total_tokens: usage
                                                                    .get("total_tokens")
                                                                    .and_then(|t| t.as_u64())
                                                                    .unwrap_or(0)
                                                                    as u32,
                                                            })
                                                        } else {
                                                            None
                                                        };

                                                    return Ok(StreamData::new(
                                                        chunk.clone(),
                                                        usage,
                                                        content,
                                                    ));
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
    async fn test_mistralai_generate() {
        let mistral = MistralAI::new();

        let res = mistral
            .generate(&[Message::new_human_message("Hello!")])
            .await
            .unwrap();

        println!("{:?}", res)
    }

    #[test]
    #[ignore]
    async fn test_mistralai_stream() {
        let mistral = MistralAI::new();
        let mut stream = mistral
            .stream(&[Message::new_human_message("Hello!")])
            .await
            .unwrap();
        while let Some(data) = stream.next().await {
            match data {
                Ok(value) => value.to_stdout().unwrap(),
                Err(e) => panic!("Error invoking MistralAI: {:?}", e),
            }
        }
    }
}
