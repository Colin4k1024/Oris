use std::pin::Pin;

use async_trait::async_trait;
use futures::Stream;

use crate::schemas::{Message, StreamData};

use super::{invocation_config::InvocationConfig, options::CallOptions, GenerateResult, LLMError};

#[async_trait]
pub trait LLM: Sync + Send + LLMClone {
    async fn generate(&self, messages: &[Message]) -> Result<GenerateResult, LLMError>;
    async fn invoke(&self, prompt: &str) -> Result<String, LLMError> {
        self.generate(&[Message::new_human_message(prompt)])
            .await
            .map(|res| res.generation)
    }
    async fn stream(
        &self,
        _messages: &[Message],
    ) -> Result<Pin<Box<dyn Stream<Item = Result<StreamData, LLMError>> + Send>>, LLMError>;

    /// This is usefull when you want to create a chain and override
    /// LLM options
    fn add_options(&mut self, _options: CallOptions) {
        // No action taken
    }
    //This is usefull when using non chat models
    fn messages_to_string(&self, messages: &[Message]) -> String {
        messages
            .iter()
            .map(|m| format!("{:?}: {}", m.message_type, m.content))
            .collect::<Vec<String>>()
            .join("\n")
    }

    /// Invoke the model with a prompt and optional invocation config.
    ///
    /// This is a convenience method that combines `invoke()` with invocation config.
    /// The default implementation ignores the config and calls `invoke()`.
    /// Individual model implementations can override this to use the config.
    async fn invoke_with_config(
        &self,
        prompt: &str,
        _config: Option<&InvocationConfig>,
    ) -> Result<String, LLMError> {
        self.invoke(prompt).await
    }

    /// Generate a response with optional invocation config.
    ///
    /// The default implementation ignores the config and calls `generate()`.
    /// Individual model implementations can override this to use the config.
    async fn generate_with_config(
        &self,
        messages: &[Message],
        _config: Option<&InvocationConfig>,
    ) -> Result<GenerateResult, LLMError> {
        self.generate(messages).await
    }

    /// Batch process multiple prompts.
    ///
    /// Processes multiple prompts in sequence and returns their results.
    /// For parallel processing, implementations can override this method.
    async fn batch(&self, prompts: &[&str]) -> Result<Vec<Result<String, LLMError>>, LLMError> {
        let mut results = Vec::new();
        for prompt in prompts {
            let result = self.invoke(prompt).await;
            results.push(result);
        }
        Ok(results)
    }

    /// Batch process multiple message sets.
    ///
    /// Processes multiple message sets in sequence and returns their results.
    async fn batch_generate(
        &self,
        message_sets: &[&[Message]],
    ) -> Result<Vec<Result<GenerateResult, LLMError>>, LLMError> {
        let mut results = Vec::new();
        for messages in message_sets {
            let result = self.generate(messages).await;
            results.push(result);
        }
        Ok(results)
    }
}

pub trait LLMClone {
    fn clone_box(&self) -> Box<dyn LLM>;
}

impl<T> LLMClone for T
where
    T: 'static + LLM + Clone,
{
    fn clone_box(&self) -> Box<dyn LLM> {
        Box::new(self.clone())
    }
}

impl<L> From<L> for Box<dyn LLM>
where
    L: 'static + LLM,
{
    fn from(llm: L) -> Self {
        Box::new(llm)
    }
}
