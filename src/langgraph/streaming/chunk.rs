use serde_json::Value;

use crate::langgraph::state::{State, StateUpdate};

use super::metadata::{DebugInfo, MessageChunk};

/// Stream chunk - represents a single item in a stream
///
/// Different stream modes produce different types of chunks.
#[derive(Clone, Debug)]
pub enum StreamChunk<S: State> {
    /// Full state value (values mode)
    Values { state: S },
    /// State update (updates mode)
    Updates { node: String, update: StateUpdate },
    /// LLM message chunk (messages mode)
    Messages { chunk: MessageChunk },
    /// Custom data (custom mode)
    Custom { node: String, data: Value },
    /// Debug information (debug mode)
    Debug { info: DebugInfo },
}

impl<S: State> StreamChunk<S> {
    /// Get the stream mode for this chunk
    pub fn mode(&self) -> super::mode::StreamMode {
        use super::mode::StreamMode;
        match self {
            Self::Values { .. } => StreamMode::Values,
            Self::Updates { .. } => StreamMode::Updates,
            Self::Messages { .. } => StreamMode::Messages,
            Self::Custom { .. } => StreamMode::Custom,
            Self::Debug { .. } => StreamMode::Debug,
        }
    }
}
