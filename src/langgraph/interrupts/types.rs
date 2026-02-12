use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Interrupt information
///
/// Represents an interrupt that occurred during graph execution.
/// This is included in the result's `__interrupt__` field.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Interrupt {
    /// The value passed to `interrupt()`
    pub value: Value,
}

impl Interrupt {
    /// Create a new Interrupt
    pub fn new(value: impl Into<Value>) -> Self {
        Self {
            value: value.into(),
        }
    }
}

impl From<Value> for Interrupt {
    fn from(value: Value) -> Self {
        Self { value }
    }
}
