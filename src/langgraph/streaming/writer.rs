use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;
use tokio::sync::mpsc;

use crate::langgraph::error::LangGraphError;

/// Trait for writing custom data to the stream
///
/// Nodes can use this to send custom data that will be
/// streamed when `custom` mode is enabled.
#[async_trait]
pub trait StreamWriter: Send + Sync {
    /// Write custom data to the stream
    async fn write(&self, data: Value) -> Result<(), LangGraphError>;
}

/// Channel-based StreamWriter implementation
///
/// Uses an async channel to send custom data to the stream.
pub struct ChannelStreamWriter {
    sender: mpsc::UnboundedSender<Value>,
}

impl ChannelStreamWriter {
    /// Create a new ChannelStreamWriter
    pub fn new(sender: mpsc::UnboundedSender<Value>) -> Self {
        Self { sender }
    }
}

#[async_trait]
impl StreamWriter for ChannelStreamWriter {
    async fn write(&self, data: Value) -> Result<(), LangGraphError> {
        self.sender.send(data).map_err(|_| {
            LangGraphError::StreamingError("Failed to send custom data".to_string())
        })?;
        Ok(())
    }
}

/// Arc-wrapped StreamWriter for sharing
pub type StreamWriterBox = Arc<dyn StreamWriter>;

/// Helper function to create a stream writer channel pair
pub fn create_stream_writer() -> (StreamWriterBox, mpsc::UnboundedReceiver<Value>) {
    let (sender, receiver) = mpsc::unbounded_channel();
    let writer = Arc::new(ChannelStreamWriter::new(sender)) as StreamWriterBox;
    (writer, receiver)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time::{sleep, Duration};

    #[tokio::test]
    async fn test_channel_stream_writer() {
        let (writer, mut receiver) = create_stream_writer();

        let data = serde_json::json!({"test": "data"});
        writer.write(data.clone()).await.unwrap();

        let received = receiver.recv().await.unwrap();
        assert_eq!(received, data);
    }

    #[tokio::test]
    async fn test_stream_writer_multiple_writes() {
        let (writer, mut receiver) = create_stream_writer();

        writer.write(serde_json::json!({"first": 1})).await.unwrap();
        writer
            .write(serde_json::json!({"second": 2}))
            .await
            .unwrap();

        let first = receiver.recv().await.unwrap();
        assert_eq!(first, serde_json::json!({"first": 1}));

        let second = receiver.recv().await.unwrap();
        assert_eq!(second, serde_json::json!({"second": 2}));
    }
}
