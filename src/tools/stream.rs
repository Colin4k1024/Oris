/// Stream writer for real-time updates from tools.
///
/// Tools can use the stream writer to provide progress updates
/// and real-time feedback as they execute.
pub trait StreamWriter: Send + Sync {
    /// Write a message to the stream
    fn write(&self, message: &str);
}

/// Simple stream writer that collects messages.
#[derive(Clone)]
pub struct CollectingStreamWriter {
    messages: std::sync::Arc<tokio::sync::Mutex<Vec<String>>>,
}

impl CollectingStreamWriter {
    pub fn new() -> Self {
        Self {
            messages: std::sync::Arc::new(tokio::sync::Mutex::new(Vec::new())),
        }
    }

    pub async fn get_messages(&self) -> Vec<String> {
        let messages = self.messages.lock().await;
        messages.clone()
    }

    pub async fn clear(&self) {
        let mut messages = self.messages.lock().await;
        messages.clear();
    }
}

impl Default for CollectingStreamWriter {
    fn default() -> Self {
        Self::new()
    }
}

impl StreamWriter for CollectingStreamWriter {
    fn write(&self, message: &str) {
        // Use blocking lock for sync trait method
        let messages = self.messages.try_lock();
        if let Ok(mut msgs) = messages {
            msgs.push(message.to_string());
        }
    }
}

/// Stream writer that writes to stdout.
pub struct StdoutStreamWriter;

impl StreamWriter for StdoutStreamWriter {
    fn write(&self, message: &str) {
        println!("[Tool] {}", message);
    }
}
