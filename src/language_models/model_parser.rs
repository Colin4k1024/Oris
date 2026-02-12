use crate::language_models::LLMError;

/// Parsed model information from a model string.
#[derive(Debug, Clone)]
pub struct ParsedModel {
    pub provider: Option<String>,
    pub model: String,
    pub azure_deployment: Option<String>,
}

/// Parse a model string into provider and model name.
///
/// Supports formats:
/// - Simple: "gpt-4o-mini" (auto-detect provider)
/// - Provider:model: "openai:gpt-4o-mini", "azure_openai:gpt-4.1"
/// - Azure with deployment: "azure_openai:gpt-4.1" (deployment specified separately)
///
/// # Example
/// ```rust,ignore
/// let parsed = parse_model_string("openai:gpt-4o-mini")?;
/// assert_eq!(parsed.provider, Some("openai".to_string()));
/// assert_eq!(parsed.model, "gpt-4o-mini");
/// ```
pub fn parse_model_string(model: &str) -> Result<ParsedModel, LLMError> {
    if model.is_empty() {
        return Err(LLMError::OtherError(
            "Model string cannot be empty".to_string(),
        ));
    }

    // Check for provider:model format
    if let Some(colon_pos) = model.find(':') {
        let provider = model[..colon_pos].to_string();
        let model_name = model[colon_pos + 1..].to_string();

        if model_name.is_empty() {
            return Err(LLMError::OtherError(format!(
                "Model name cannot be empty in format 'provider:model': {}",
                model
            )));
        }

        Ok(ParsedModel {
            provider: Some(provider),
            model: model_name,
            azure_deployment: None,
        })
    } else {
        // Simple format - no provider specified, will auto-detect
        Ok(ParsedModel {
            provider: None,
            model: model.to_string(),
            azure_deployment: None,
        })
    }
}

impl ParsedModel {
    pub fn with_azure_deployment(mut self, deployment: String) -> Self {
        self.azure_deployment = Some(deployment);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_model() {
        let parsed = parse_model_string("gpt-4o-mini").unwrap();
        assert_eq!(parsed.provider, None);
        assert_eq!(parsed.model, "gpt-4o-mini");
    }

    #[test]
    fn test_parse_provider_model() {
        let parsed = parse_model_string("openai:gpt-4o-mini").unwrap();
        assert_eq!(parsed.provider, Some("openai".to_string()));
        assert_eq!(parsed.model, "gpt-4o-mini");
    }

    #[test]
    fn test_parse_azure_model() {
        let parsed = parse_model_string("azure_openai:gpt-4.1").unwrap();
        assert_eq!(parsed.provider, Some("azure_openai".to_string()));
        assert_eq!(parsed.model, "gpt-4.1");
    }

    #[test]
    fn test_parse_empty_string() {
        let result = parse_model_string("");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_invalid_format() {
        let result = parse_model_string("provider:");
        assert!(result.is_err());
    }
}
