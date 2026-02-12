use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::schemas::StreamData;

/// Metadata for message streaming events
///
/// Contains information about the LLM invocation and graph node
/// where the message chunk was generated.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MessageMetadata {
    /// The name of the graph node where the LLM was invoked
    pub langgraph_node: String,

    /// Tags associated with the LLM invocation (if any)
    pub tags: Vec<String>,

    /// Additional metadata about the LLM invocation
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

impl MessageMetadata {
    /// Create a new MessageMetadata
    pub fn new(node: impl Into<String>) -> Self {
        Self {
            langgraph_node: node.into(),
            tags: Vec::new(),
            extra: HashMap::new(),
        }
    }

    /// Create with tags
    pub fn with_tags(node: impl Into<String>, tags: Vec<String>) -> Self {
        Self {
            langgraph_node: node.into(),
            tags,
            extra: HashMap::new(),
        }
    }

    /// Add extra metadata
    pub fn with_extra(mut self, key: String, value: Value) -> Self {
        self.extra.insert(key, value);
        self
    }
}

/// Debug information for debug stream mode
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DebugInfo {
    /// Event type (e.g., "NodeStart", "NodeEnd", "MessageChunk")
    pub event_type: String,

    /// Node name (if applicable)
    pub node: Option<String>,

    /// Additional debug information
    #[serde(flatten)]
    pub info: HashMap<String, Value>,
}

impl DebugInfo {
    /// Create a new DebugInfo
    pub fn new(event_type: impl Into<String>) -> Self {
        Self {
            event_type: event_type.into(),
            node: None,
            info: HashMap::new(),
        }
    }

    /// Create with node name
    pub fn with_node(event_type: impl Into<String>, node: impl Into<String>) -> Self {
        Self {
            event_type: event_type.into(),
            node: Some(node.into()),
            info: HashMap::new(),
        }
    }

    /// Add debug information
    pub fn with_info(mut self, key: String, value: Value) -> Self {
        self.info.insert(key, value);
        self
    }
}

/// Message chunk with metadata
///
/// Represents a single token or message segment from an LLM
/// along with its metadata.
#[derive(Clone, Debug)]
pub struct MessageChunk {
    /// The stream data (token content)
    pub chunk: StreamData,

    /// Metadata about the LLM invocation
    pub metadata: MessageMetadata,
}

impl MessageChunk {
    /// Create a new MessageChunk
    pub fn new(chunk: StreamData, metadata: MessageMetadata) -> Self {
        Self { chunk, metadata }
    }
}
