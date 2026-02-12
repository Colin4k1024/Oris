use serde_json::Value;

/// Error type for interrupts
///
/// When `interrupt()` is called and there's no resume value,
/// it returns this error to signal that execution should be paused.
#[derive(thiserror::Error, Debug, Clone)]
#[error("Interrupt: {0}")]
pub struct InterruptError(pub Value);

impl InterruptError {
    /// Create a new InterruptError
    pub fn new(value: impl Into<Value>) -> Self {
        Self(value.into())
    }

    /// Get the interrupt value
    pub fn value(&self) -> &Value {
        &self.0
    }

    /// Convert to LangGraphError
    pub fn into_langgraph_error(self) -> crate::langgraph::error::LangGraphError {
        crate::langgraph::error::LangGraphError::InterruptError(self)
    }
}
