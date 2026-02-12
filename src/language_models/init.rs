use crate::{
    language_models::{llm::LLM, model_parser::parse_model_string, options::CallOptions, LLMError},
    llm::{
        claude::Claude,
        deepseek::Deepseek,
        openai::{AzureConfig, OpenAI, OpenAIConfig},
        qwen::Qwen,
    },
};

#[cfg(feature = "ollama")]
use crate::llm::Ollama;

#[cfg(feature = "mistralai")]
use crate::llm::MistralAI;

#[cfg(feature = "gemini")]
use crate::llm::Gemini;

#[cfg(feature = "bedrock")]
use crate::llm::Bedrock;

use crate::llm::HuggingFace;

/// Initialize a chat model from a model string with optional parameters.
///
/// This is the unified interface for creating LLM instances, similar to
/// LangChain Python's `init_chat_model()` function.
///
/// # Supported Formats
///
/// - Simple format: "gpt-4o-mini" (auto-detects provider)
/// - Provider:model format: "openai:gpt-4o-mini", "azure_openai:gpt-4.1"
///
/// # Supported Models
///
/// - OpenAI: "gpt-3.5-turbo", "gpt-4", "gpt-4o", "gpt-4o-mini", "gpt-4-turbo"
/// - Claude: "claude-3-opus", "claude-3-sonnet", "claude-3-haiku", "claude-3-5-sonnet", "claude-sonnet-4-5-20250929"
/// - Qwen: "qwen-plus", "qwen-max", "qwen-turbo", "qwen"
/// - Deepseek: "deepseek-chat", "deepseek-reasoner", "deepseek"
/// - MistralAI: "mistral-small-latest", "mistral-medium-latest", "mistral-large-latest", etc. (requires "mistralai" feature)
/// - Google Gemini: "gemini-1.5-flash", "gemini-1.5-pro", "gemini-2.0-flash", etc. (requires "gemini" feature)
/// - AWS Bedrock: "anthropic.claude-3-5-sonnet-20240620-v1:0", "meta.llama3-70b-instruct-v1:0", etc. (requires "bedrock" feature)
/// - HuggingFace: "microsoft/Phi-3-mini-4k-instruct", "meta-llama/Llama-3.1-8B-Instruct", etc.
/// - Ollama: "llama3", "mistral", "codellama", etc. (requires "ollama" feature)
///
/// # Parameters
///
/// - `model`: Model identifier string
/// - `temperature`: Optional temperature (0.0-2.0 for most models)
/// - `max_tokens`: Optional maximum tokens in response
/// - `timeout`: Optional request timeout in seconds
/// - `max_retries`: Optional maximum retry attempts
/// - `api_key`: Optional API key override
/// - `base_url`: Optional base URL override (for proxies)
/// - `azure_deployment`: Optional Azure deployment ID (for Azure OpenAI)
///
/// # Example
/// ```rust,ignore
/// use oris::language_models::init_chat_model;
///
/// let model = init_chat_model(
///     "gpt-4o-mini",
///     Some(0.7),
///     Some(1000),
///     None,
///     None,
///     None,
///     None,
///     None,
/// )
/// .await?;
///
/// let response = model.invoke("Hello!").await?;
/// ```
pub async fn init_chat_model(
    model: &str,
    temperature: Option<f32>,
    max_tokens: Option<u32>,
    timeout: Option<u64>,
    max_retries: Option<u32>,
    api_key: Option<String>,
    base_url: Option<String>,
    azure_deployment: Option<String>,
) -> Result<Box<dyn LLM>, LLMError> {
    let parsed = parse_model_string(model)?;
    let provider = parsed.provider.as_deref();

    // Build CallOptions
    let mut options = CallOptions::new();
    if let Some(temp) = temperature {
        options = options.with_temperature(temp);
    }
    if let Some(max) = max_tokens {
        options = options.with_max_tokens(max);
    }
    if let Some(to) = timeout {
        options = options.with_timeout(to);
    }
    if let Some(retries) = max_retries {
        options = options.with_max_retries(retries);
    }
    if let Some(key) = api_key {
        options = options.with_api_key(key);
    }
    if let Some(url) = base_url {
        options = options.with_base_url(url);
    }

    // Determine provider and create model
    let model_name = parsed.model.as_str();
    let model_lower = model_name.to_lowercase();

    // Handle Azure OpenAI
    if provider == Some("azure_openai")
        || (provider.is_none() && model_lower.starts_with("gpt-") && azure_deployment.is_some())
    {
        let mut config = AzureConfig::default();
        if let Some(key) = &options.api_key {
            config = config.with_api_key(key.clone());
        }
        if let Some(url) = &options.base_url {
            config = config.with_api_base(url.clone());
        }
        if let Some(deployment) = azure_deployment {
            config = config.with_deployment_id(deployment);
        }
        let llm: OpenAI<AzureConfig> = OpenAI::new(config)
            .with_model(model_name)
            .with_options(options);
        return Ok(Box::new(llm));
    }

    // OpenAI models
    if provider == Some("openai") || (provider.is_none() && model_lower.starts_with("gpt-")) {
        let mut config = OpenAIConfig::default();
        if let Some(key) = &options.api_key {
            config = config.with_api_key(key.clone());
        }
        // Note: OpenAIConfig from async_openai doesn't support base_url directly
        // Base URL can be set via environment variable OPENAI_API_BASE
        let llm: OpenAI<OpenAIConfig> = OpenAI::new(config)
            .with_model(model_name)
            .with_options(options);
        return Ok(Box::new(llm));
    }

    // Claude models
    if provider == Some("anthropic")
        || provider == Some("claude")
        || (provider.is_none()
            && (model_lower.starts_with("claude-") || model_lower.starts_with("claude")))
    {
        let mut llm = Claude::default().with_model(model_name);
        if let Some(key) = &options.api_key {
            llm = llm.with_api_key(key.clone());
        }
        // Note: Claude doesn't support base_url override in current implementation
        llm = llm.with_options(options);
        return Ok(Box::new(llm));
    }

    // Qwen models
    if provider == Some("qwen")
        || (provider.is_none() && (model_lower.starts_with("qwen-") || model_lower == "qwen"))
    {
        let mut llm = Qwen::default().with_model(model_name);
        if let Some(key) = &options.api_key {
            llm = llm.with_api_key(key.clone());
        }
        if let Some(url) = &options.base_url {
            llm = llm.with_base_url(url.clone());
        }
        llm = llm.with_options(options);
        return Ok(Box::new(llm));
    }

    // Deepseek models
    if provider == Some("deepseek")
        || (provider.is_none()
            && (model_lower.starts_with("deepseek-") || model_lower == "deepseek"))
    {
        let mut llm = Deepseek::default().with_model(model_name);
        if let Some(key) = &options.api_key {
            llm = llm.with_api_key(key.clone());
        }
        if let Some(url) = &options.base_url {
            llm = llm.with_base_url(url.clone());
        }
        llm = llm.with_options(options);
        return Ok(Box::new(llm));
    }

    // MistralAI models
    #[cfg(feature = "mistralai")]
    {
        if provider == Some("mistralai")
            || provider == Some("mistral")
            || (provider.is_none()
                && (model_lower.starts_with("mistral-")
                    || model_lower.starts_with("mixtral-")
                    || model_lower.starts_with("pixtral-")))
        {
            let mut llm = MistralAI::default().with_model(model_name);
            if let Some(key) = &options.api_key {
                llm = llm.with_api_key(key.clone());
            }
            if let Some(url) = &options.base_url {
                llm = llm.with_base_url(url.clone());
            }
            llm = llm.with_options(options);
            return Ok(Box::new(llm));
        }
    }

    // Google Gemini models
    #[cfg(feature = "gemini")]
    {
        if provider == Some("gemini")
            || provider == Some("google")
            || provider == Some("google_genai")
            || (provider.is_none()
                && (model_lower.starts_with("gemini-") || model_lower.starts_with("gemini")))
        {
            let mut llm = Gemini::default().with_model(model_name);
            if let Some(key) = &options.api_key {
                llm = llm.with_api_key(key.clone());
            }
            if let Some(url) = &options.base_url {
                llm = llm.with_base_url(url.clone());
            }
            llm = llm.with_options(options);
            return Ok(Box::new(llm));
        }
    }

    // AWS Bedrock models
    #[cfg(feature = "bedrock")]
    {
        if provider == Some("bedrock")
            || provider == Some("aws_bedrock")
            || (provider.is_none()
                && (model_lower.contains("anthropic.claude")
                    || model_lower.contains("meta.llama")
                    || model_lower.contains("amazon.titan")))
        {
            // Note: Bedrock requires async initialization
            // For now, we'll create it synchronously which may fail
            // In production, consider using a factory pattern
            let bedrock = Bedrock::new().await?;
            let bedrock = bedrock.with_model(model_name);
            // Note: Bedrock doesn't support all CallOptions directly
            // Some options may need to be converted
            let bedrock = bedrock.with_options(options);
            return Ok(Box::new(bedrock));
        }
    }

    // HuggingFace models
    {
        if provider == Some("huggingface")
            || provider == Some("hf")
            || (provider.is_none() && model_lower.contains("/"))
        {
            let mut llm = HuggingFace::default().with_model(model_name);
            if let Some(key) = &options.api_key {
                llm = llm.with_api_key(key.clone());
            }
            if let Some(url) = &options.base_url {
                llm = llm.with_base_url(url.clone());
            }
            llm = llm.with_options(options);
            return Ok(Box::new(llm));
        }
    }

    // Ollama models (default for unrecognized patterns)
    // Note: Ollama uses a different options structure (GenerationOptions)
    // and doesn't support base_url override in the same way as other providers
    #[cfg(feature = "ollama")]
    {
        let mut gen_options = ollama_rs::generation::options::GenerationOptions::default();

        // Convert CallOptions to GenerationOptions where applicable
        if let Some(temp) = options.temperature {
            gen_options = gen_options.temperature(temp);
        }
        if let Some(max_tokens) = options.max_tokens {
            gen_options = gen_options.num_predict(max_tokens as i32);
        }
        if let Some(top_p) = options.top_p {
            gen_options = gen_options.top_p(top_p);
        }
        if let Some(top_k) = options.top_k {
            gen_options = gen_options.top_k(top_k as u32);
        }
        if let Some(seed) = options.seed {
            gen_options = gen_options.seed(seed as i32);
        }

        let llm = Ollama::default()
            .with_model(model_name)
            .with_options(gen_options);
        return Ok(Box::new(llm));
    }

    #[cfg(not(feature = "ollama"))]
    {
        #[cfg(feature = "mistralai")]
        {
            Err(LLMError::OtherError(format!(
                "Unsupported model: {}. Supported providers: openai, azure_openai, anthropic, qwen, deepseek, mistralai. Enable 'ollama' feature for Ollama models.",
                model
            )))
        }
        #[cfg(not(feature = "mistralai"))]
        {
            Err(LLMError::OtherError(format!(
                "Unsupported model: {}. Supported providers: openai, azure_openai, anthropic, qwen, deepseek. Enable 'mistralai' feature for MistralAI models. Enable 'ollama' feature for Ollama models.",
                model
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_init_chat_model_openai() {
        let result = init_chat_model("gpt-4o-mini", None, None, None, None, None, None, None).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_init_chat_model_with_provider() {
        let result = init_chat_model(
            "openai:gpt-4o-mini",
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_init_chat_model_with_params() {
        let result = init_chat_model(
            "gpt-4o-mini",
            Some(0.7),
            Some(1000),
            Some(30),
            Some(3),
            None,
            None,
            None,
        )
        .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_init_chat_model_claude() {
        let result = init_chat_model(
            "claude-3-5-sonnet-20240620",
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_init_chat_model_invalid() {
        let result = init_chat_model(
            "invalid-model-xyz",
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .await;
        assert!(result.is_err());
    }
}
