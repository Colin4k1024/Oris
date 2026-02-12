use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Command for resuming graph execution after an interrupt
///
/// Used to resume execution with a value or route to a specific node.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum Command {
    /// Resume execution with a value
    ///
    /// The value becomes the return value of the `interrupt()` call.
    #[serde(rename = "resume")]
    Resume {
        /// The value to pass back to the interrupt() call
        #[serde(rename = "resume")]
        value: Value,
    },
    /// Go to a specific node
    #[serde(rename = "goto")]
    Goto {
        /// The node name to route to
        node: String,
    },
}

impl Command {
    /// Create a resume command with a value
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use oris::langgraph::interrupts::Command;
    ///
    /// let cmd = Command::resume(true);
    /// let cmd = Command::resume("approved");
    /// ```
    pub fn resume(value: impl Into<Value>) -> Self {
        Self::Resume {
            value: value.into(),
        }
    }

    /// Create a goto command to route to a specific node
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use oris::langgraph::interrupts::Command;
    ///
    /// let cmd = Command::goto("approve_node");
    /// ```
    pub fn goto(node: impl Into<String>) -> Self {
        Self::Goto { node: node.into() }
    }

    /// Check if this is a resume command
    pub fn is_resume(&self) -> bool {
        matches!(self, Self::Resume { .. })
    }

    /// Check if this is a goto command
    pub fn is_goto(&self) -> bool {
        matches!(self, Self::Goto { .. })
    }

    /// Get the resume value if this is a resume command
    pub fn resume_value(&self) -> Option<&Value> {
        match self {
            Self::Resume { value } => Some(value),
            _ => None,
        }
    }

    /// Get the node name if this is a goto command
    pub fn goto_node(&self) -> Option<&str> {
        match self {
            Self::Goto { node } => Some(node),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_resume() {
        let cmd = Command::resume(true);
        assert!(cmd.is_resume());
        assert_eq!(cmd.resume_value(), Some(&serde_json::json!(true)));
    }

    #[test]
    fn test_command_goto() {
        let cmd = Command::goto("node1");
        assert!(cmd.is_goto());
        assert_eq!(cmd.goto_node(), Some("node1"));
    }
}
