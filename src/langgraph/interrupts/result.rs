use serde_json::Value;

use crate::langgraph::state::State;

use super::types::Interrupt;

/// Result of graph invocation that may contain interrupt information
///
/// When an interrupt occurs, the result includes the `__interrupt__` field
/// containing information about the interrupt.
#[derive(Debug, Clone)]
pub struct InvokeResult<S: State> {
    /// The final state (or state at interrupt point)
    pub state: S,
    /// Interrupt information (if an interrupt occurred)
    pub interrupt: Option<Vec<Interrupt>>,
}

impl<S: State> InvokeResult<S> {
    /// Create a new InvokeResult with state only
    pub fn new(state: S) -> Self {
        Self {
            state,
            interrupt: None,
        }
    }

    /// Create a new InvokeResult with interrupt information
    pub fn with_interrupt(state: S, interrupt: Vec<Interrupt>) -> Self {
        Self {
            state,
            interrupt: Some(interrupt),
        }
    }

    /// Convert to JSON format (similar to Python's result format)
    ///
    /// The result will have the state as the main object, with
    /// `__interrupt__` field if interrupts occurred.
    pub fn to_json(&self) -> Result<Value, crate::langgraph::error::LangGraphError> {
        let mut result = serde_json::to_value(&self.state)
            .map_err(crate::langgraph::error::LangGraphError::SerializationError)?;

        if let Some(ref interrupts) = self.interrupt {
            result["__interrupt__"] = serde_json::to_value(interrupts)
                .map_err(crate::langgraph::error::LangGraphError::SerializationError)?;
        }

        Ok(result)
    }

    /// Check if this result contains an interrupt
    pub fn has_interrupt(&self) -> bool {
        self.interrupt.is_some()
    }

    /// Get the interrupt information
    pub fn interrupt(&self) -> Option<&Vec<Interrupt>> {
        self.interrupt.as_ref()
    }
}

impl<S: State> From<S> for InvokeResult<S> {
    fn from(state: S) -> Self {
        Self::new(state)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::langgraph::state::MessagesState;

    #[test]
    fn test_invoke_result() {
        let state = MessagesState::new();
        let result = InvokeResult::new(state);
        assert!(!result.has_interrupt());
    }

    #[test]
    fn test_invoke_result_with_interrupt() {
        let state = MessagesState::new();
        let interrupt = Interrupt::new(serde_json::json!("test"));
        let result = InvokeResult::with_interrupt(state, vec![interrupt]);
        assert!(result.has_interrupt());
    }
}
