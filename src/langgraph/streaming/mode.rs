/// Stream mode for LangGraph streaming
///
/// Determines what type of data is streamed during graph execution.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StreamMode {
    /// Stream the full state value after each step
    Values,
    /// Stream state updates (deltas) after each node execution
    Updates,
    /// Stream LLM tokens from LLM nodes
    Messages,
    /// Stream custom data from nodes
    Custom,
    /// Stream debug information (all events)
    Debug,
}

impl StreamMode {
    /// Parse stream mode from string
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "values" => Some(Self::Values),
            "updates" => Some(Self::Updates),
            "messages" => Some(Self::Messages),
            "custom" => Some(Self::Custom),
            "debug" => Some(Self::Debug),
            _ => None,
        }
    }

    /// Convert stream mode to string
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Values => "values",
            Self::Updates => "updates",
            Self::Messages => "messages",
            Self::Custom => "custom",
            Self::Debug => "debug",
        }
    }
}

impl std::fmt::Display for StreamMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stream_mode_from_str() {
        assert_eq!(StreamMode::from_str("values"), Some(StreamMode::Values));
        assert_eq!(StreamMode::from_str("updates"), Some(StreamMode::Updates));
        assert_eq!(StreamMode::from_str("messages"), Some(StreamMode::Messages));
        assert_eq!(StreamMode::from_str("custom"), Some(StreamMode::Custom));
        assert_eq!(StreamMode::from_str("debug"), Some(StreamMode::Debug));
        assert_eq!(StreamMode::from_str("invalid"), None);
    }

    #[test]
    fn test_stream_mode_as_str() {
        assert_eq!(StreamMode::Values.as_str(), "values");
        assert_eq!(StreamMode::Updates.as_str(), "updates");
        assert_eq!(StreamMode::Messages.as_str(), "messages");
        assert_eq!(StreamMode::Custom.as_str(), "custom");
        assert_eq!(StreamMode::Debug.as_str(), "debug");
    }
}
