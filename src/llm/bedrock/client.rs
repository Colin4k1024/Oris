use crate::{
    language_models::{llm::LLM, options::CallOptions, GenerateResult, LLMError, TokenUsage},
    llm::BedrockError,
    schemas::{Message, StreamData},
};
use async_trait::async_trait;
use aws_sdk_bedrockruntime::Client as BedrockClient;
use futures::{Stream, StreamExt};
use serde_json::Value;
use std::pin::Pin;
use std::sync::Arc;

use super::models::{ApiResponse, BedrockMessage, Payload};

/// AWS Bedrock client
#[derive(Clone)]
pub struct Bedrock {
    model: String,
    options: CallOptions,
    client: Arc<BedrockClient>,
    region: String,
}

impl Default for Bedrock {
    fn default() -> Self {
        // Note: This requires async initialization, use Bedrock::new() instead
        // This is provided for compatibility with the builder pattern
        panic!("Bedrock::default() is not supported. Use Bedrock::new().await instead");
    }
}

impl Bedrock {
    /// Create a new Bedrock client with default settings
    pub async fn new() -> Result<Self, LLMError> {
        let config = aws_config::load_from_env().await;
        let client = BedrockClient::new(&config);
        let region = config
            .region()
            .map(|r| r.to_string())
            .unwrap_or_else(|| "us-east-1".to_string());

        Ok(Self {
            model: "anthropic.claude-3-5-sonnet-20240620-v1:0".to_string(),
            options: CallOptions::default(),
            client: Arc::new(client),
            region,
        })
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

    /// Set region
    pub fn with_region<S: Into<String>>(mut self, region: S) -> Self {
        self.region = region.into();
        self
    }

    /// Generates text using the Bedrock API
    async fn generate(&self, messages: &[Message]) -> Result<GenerateResult, LLMError> {
        let payload = self.build_payload(messages);
        let payload_json = serde_json::to_string(&payload).map_err(|e| LLMError::SerdeError(e))?;
        let payload_bytes = payload_json.into_bytes();

        let response = self
            .client
            .invoke_model()
            .model_id(&self.model)
            .content_type("application/json")
            .body(payload_bytes.into())
            .send()
            .await
            .map_err(|e| LLMError::BedrockError(BedrockError::SystemError(e.to_string())))?;

        let body_bytes = response.body.into_inner();
        let body_str =
            String::from_utf8(body_bytes).map_err(|e| LLMError::OtherError(e.to_string()))?;

        // Parse response - Bedrock uses different formats for different models
        // For Anthropic Claude: {"content": [{"text": "..."}], "usage": {...}}
        // For other models: similar structure
        let api_response: ApiResponse =
            serde_json::from_str(&body_str).map_err(|e| LLMError::SerdeError(e))?;

        let generation = api_response
            .content
            .first()
            .map(|c| c.text.clone())
            .unwrap_or_default();

        let tokens = api_response.usage.map(|usage| TokenUsage {
            prompt_tokens: usage.input_tokens,
            completion_tokens: usage.output_tokens,
            total_tokens: usage.input_tokens + usage.output_tokens,
        });

        Ok(GenerateResult { tokens, generation })
    }

    /// Builds the API payload from messages
    fn build_payload(&self, messages: &[Message]) -> Payload {
        // Determine if this is an Anthropic model (Claude)
        let is_anthropic = self.model.contains("anthropic.claude");

        Payload {
            anthropic_version: if is_anthropic {
                Some("bedrock-2023-05-31".to_string())
            } else {
                None
            },
            messages: messages
                .iter()
                .map(BedrockMessage::from_message)
                .collect::<Vec<_>>(),
            max_tokens: self.options.max_tokens,
            temperature: self.options.temperature,
            top_p: self.options.top_p,
            top_k: self.options.top_k.map(|k| k as u32),
            stop_sequences: self.options.stop_words.clone(),
        }
    }
}

#[async_trait]
impl LLM for Bedrock {
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
        let payload = self.build_payload(messages);
        let payload_json = serde_json::to_string(&payload).map_err(|e| LLMError::SerdeError(e))?;
        let payload_bytes = payload_json.into_bytes();

        let mut response = self
            .client
            .invoke_model_with_response_stream()
            .model_id(&self.model)
            .content_type("application/json")
            .body(payload_bytes.into())
            .send()
            .await
            .map_err(|e| LLMError::BedrockError(BedrockError::SystemError(e.to_string())))?;

        use aws_sdk_bedrockruntime::types::ResponseStream;

        let stream = async_stream::stream! {
            while let Ok(Some(event_result)) = response.body.recv().await {
                match event_result {
                    ResponseStream::Chunk(chunk) => {
                        // Extract bytes from the chunk
                        if let Some(bytes) = chunk.bytes {
                            match String::from_utf8(bytes.into_inner()) {
                                Ok(text_chunk) => {
                                    if !text_chunk.is_empty() {
                                        // Parse the JSON response to extract text content
                                        // Bedrock returns JSON chunks with structure like:
                                        // {"delta": {"text": "..."}} for Anthropic Claude models
                                        // or {"contentBlockDelta": {"delta": {"text": "..."}}} for other formats
                                        if let Ok(json_value) = serde_json::from_str::<Value>(&text_chunk) {
                                            // Try to extract text from various possible structures
                                            let text = json_value
                                                .get("delta")
                                                .and_then(|d| d.get("text"))
                                                .and_then(|t| t.as_str())
                                                .or_else(|| {
                                                    json_value
                                                        .get("contentBlockDelta")
                                                        .and_then(|c| c.get("delta"))
                                                        .and_then(|d| d.get("text"))
                                                        .and_then(|t| t.as_str())
                                                })
                                                .or_else(|| {
                                                    json_value
                                                        .get("content")
                                                        .and_then(|c| c.as_str())
                                                })
                                                .unwrap_or("");

                                            if !text.is_empty() {
                                                yield Ok(StreamData::new(
                                                    json_value.clone(),
                                                    None,
                                                    text,
                                                ));
                                            }
                                        } else {
                                            // If not JSON, treat as plain text (shouldn't happen with Bedrock, but handle gracefully)
                                            if !text_chunk.trim().is_empty() {
                                                yield Ok(StreamData::new(
                                                    Value::String(text_chunk.clone()),
                                                    None,
                                                    text_chunk,
                                                ));
                                            }
                                        }
                                    }
                                }
                                Err(e) => {
                                    yield Err(LLMError::BedrockError(BedrockError::SystemError(
                                        format!("Failed to decode UTF-8: {}", e)
                                    )));
                                }
                            }
                        }
                    }
                    _ => {
                        // Ignore other event types (internal server exceptions, etc.)
                        // These are typically handled at a higher level
                    }
                }
            }
        };

        Ok(Box::pin(stream))
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
    async fn test_bedrock_generate() {
        let bedrock = Bedrock::new().await.unwrap();

        let res = bedrock
            .generate(&[Message::new_human_message("Hello!")])
            .await
            .unwrap();

        println!("{:?}", res)
    }
}
