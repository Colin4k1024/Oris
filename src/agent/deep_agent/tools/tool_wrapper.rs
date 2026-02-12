//! Wrapper to override a tool's name and/or description (e.g. for custom_tool_descriptions).

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;

use crate::error::ToolError;
use crate::tools::{Tool, ToolResult, ToolRuntime};

/// Wraps a tool and overrides its name and/or description for the agent (e.g. from
/// [DeepAgentConfig::custom_tool_descriptions](crate::agent::deep_agent::DeepAgentConfig)).
/// All other behavior (parameters, run, parse_input) is delegated to the inner tool.
pub struct ToolWithCustomDescription {
    inner: Arc<dyn Tool>,
    name_override: Option<String>,
    description_override: Option<String>,
}

impl ToolWithCustomDescription {
    /// Wrap a tool with optional name and description overrides.
    pub fn new(
        inner: Arc<dyn Tool>,
        name_override: Option<String>,
        description_override: Option<String>,
    ) -> Self {
        Self {
            inner,
            name_override,
            description_override,
        }
    }

    /// Wrap a tool with only a description override (name comes from inner).
    pub fn with_description(inner: Arc<dyn Tool>, description: impl Into<String>) -> Self {
        Self::new(inner, None, Some(description.into()))
    }
}

#[async_trait]
impl Tool for ToolWithCustomDescription {
    fn name(&self) -> String {
        self.name_override
            .clone()
            .unwrap_or_else(|| self.inner.name())
    }

    fn description(&self) -> String {
        self.description_override
            .clone()
            .unwrap_or_else(|| self.inner.description())
    }

    fn parameters(&self) -> Value {
        self.inner.parameters()
    }

    async fn parse_input(&self, input: &str) -> Value {
        self.inner.parse_input(input).await
    }

    async fn run(&self, input: Value) -> Result<String, ToolError> {
        self.inner.run(input).await
    }

    async fn run_with_runtime(
        &self,
        input: Value,
        runtime: &ToolRuntime,
    ) -> Result<ToolResult, Box<dyn std::error::Error>> {
        self.inner.run_with_runtime(input, runtime).await
    }

    fn requires_runtime(&self) -> bool {
        self.inner.requires_runtime()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    struct DummyTool;

    #[async_trait::async_trait]
    impl Tool for DummyTool {
        fn name(&self) -> String {
            "dummy".to_string()
        }
        fn description(&self) -> String {
            "Default description".to_string()
        }
        async fn run(&self, _input: Value) -> Result<String, ToolError> {
            Ok("ok".to_string())
        }
    }

    #[tokio::test]
    async fn test_wrapper_overrides_description() {
        let inner = Arc::new(DummyTool);
        let wrapped = ToolWithCustomDescription::with_description(inner, "Custom description");
        assert_eq!(wrapped.name(), "dummy");
        assert_eq!(wrapped.description(), "Custom description");
        let out = wrapped.call("{}").await.unwrap();
        assert_eq!(out, "ok");
    }

    #[tokio::test]
    async fn test_wrapper_no_override_uses_inner() {
        let inner = Arc::new(DummyTool);
        let wrapped = ToolWithCustomDescription::new(Arc::clone(&inner), None, None);
        assert_eq!(wrapped.name(), "dummy");
        assert_eq!(wrapped.description(), "Default description");
    }
}
