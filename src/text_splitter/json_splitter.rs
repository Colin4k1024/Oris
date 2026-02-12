use async_trait::async_trait;
use serde_json::Value;

use super::{TextSplitter, TextSplitterError};

/// Mode for splitting JSON
#[derive(Debug, Clone)]
pub enum JsonSplitMode {
    /// Split by JSON objects (keys at root level)
    Object,
    /// Split by JSON array elements
    Array,
    /// Split by both objects and arrays
    Both,
}

/// Configuration for JsonSplitter
#[derive(Debug, Clone)]
pub struct JsonSplitterOptions {
    /// Maximum chunk size (in characters)
    pub chunk_size: usize,
    /// Overlap between chunks (in characters)
    pub chunk_overlap: usize,
    /// How to split JSON
    pub split_mode: JsonSplitMode,
    /// Whether to include JSON path in metadata
    pub include_path: bool,
    /// Whether to trim whitespace from chunks
    pub trim_chunks: bool,
}

impl Default for JsonSplitterOptions {
    fn default() -> Self {
        Self::new()
    }
}

impl JsonSplitterOptions {
    pub fn new() -> Self {
        Self {
            chunk_size: 1000,
            chunk_overlap: 200,
            split_mode: JsonSplitMode::Both,
            include_path: true,
            trim_chunks: true,
        }
    }

    pub fn with_chunk_size(mut self, chunk_size: usize) -> Self {
        self.chunk_size = chunk_size;
        self
    }

    pub fn with_chunk_overlap(mut self, chunk_overlap: usize) -> Self {
        self.chunk_overlap = chunk_overlap;
        self
    }

    pub fn with_split_mode(mut self, split_mode: JsonSplitMode) -> Self {
        self.split_mode = split_mode;
        self
    }

    pub fn with_include_path(mut self, include_path: bool) -> Self {
        self.include_path = include_path;
        self
    }

    pub fn with_trim_chunks(mut self, trim_chunks: bool) -> Self {
        self.trim_chunks = trim_chunks;
        self
    }
}

/// JsonSplitter splits JSON documents based on JSON structure
///
/// This splitter can split JSON by objects, arrays, or both,
/// preserving the JSON structure in each chunk.
pub struct JsonSplitter {
    options: JsonSplitterOptions,
}

impl Default for JsonSplitter {
    fn default() -> Self {
        Self::new()
    }
}

impl JsonSplitter {
    /// Create a new JsonSplitter with default options
    pub fn new() -> Self {
        Self::with_options(JsonSplitterOptions::default())
    }

    /// Create a new JsonSplitter with custom options
    pub fn with_options(options: JsonSplitterOptions) -> Self {
        Self { options }
    }

    /// Create with custom chunk size
    pub fn with_chunk_size(chunk_size: usize) -> Self {
        Self::new().with_chunk_size_option(chunk_size)
    }

    /// Set chunk size
    pub fn with_chunk_size_option(mut self, chunk_size: usize) -> Self {
        self.options.chunk_size = chunk_size;
        self
    }

    /// Set chunk overlap
    pub fn with_chunk_overlap(mut self, chunk_overlap: usize) -> Self {
        self.options.chunk_overlap = chunk_overlap;
        self
    }

    /// Set split mode
    pub fn with_split_mode(mut self, split_mode: JsonSplitMode) -> Self {
        self.options.split_mode = split_mode;
        self
    }

    /// Set include path
    pub fn with_include_path(mut self, include_path: bool) -> Self {
        self.options.include_path = include_path;
        self
    }

    /// Extract JSON elements based on split mode
    fn extract_json_elements(&self, json: &Value, path: &str) -> Vec<(String, String)> {
        let mut elements = Vec::new();

        match json {
            Value::Object(map) => {
                match &self.options.split_mode {
                    JsonSplitMode::Object | JsonSplitMode::Both => {
                        for (key, value) in map {
                            let new_path = if path.is_empty() {
                                key.clone()
                            } else {
                                format!("{}.{}", path, key)
                            };

                            let json_str =
                                serde_json::to_string(value).unwrap_or_else(|_| value.to_string());

                            if self.options.include_path {
                                elements.push((format!("{}: {}", new_path, json_str), new_path));
                            } else {
                                elements.push((json_str, new_path));
                            }
                        }
                    }
                    _ => {
                        // Include entire object
                        let json_str =
                            serde_json::to_string(json).unwrap_or_else(|_| json.to_string());
                        elements.push((json_str, path.to_string()));
                    }
                }
            }
            Value::Array(arr) => {
                match &self.options.split_mode {
                    JsonSplitMode::Array | JsonSplitMode::Both => {
                        for (i, value) in arr.iter().enumerate() {
                            let new_path = if path.is_empty() {
                                format!("[{}]", i)
                            } else {
                                format!("{}[{}]", path, i)
                            };

                            let json_str =
                                serde_json::to_string(value).unwrap_or_else(|_| value.to_string());

                            if self.options.include_path {
                                elements.push((format!("{}: {}", new_path, json_str), new_path));
                            } else {
                                elements.push((json_str, new_path));
                            }
                        }
                    }
                    _ => {
                        // Include entire array
                        let json_str =
                            serde_json::to_string(json).unwrap_or_else(|_| json.to_string());
                        elements.push((json_str, path.to_string()));
                    }
                }
            }
            _ => {
                // Primitive value
                let json_str = serde_json::to_string(json).unwrap_or_else(|_| json.to_string());
                elements.push((json_str, path.to_string()));
            }
        }

        elements
    }

    /// Split JSON elements into chunks
    fn split_elements_into_chunks(&self, elements: Vec<(String, String)>) -> Vec<String> {
        let mut chunks = Vec::new();
        let mut current_chunk = String::new();

        for (text, _path) in elements {
            let text_trimmed = if self.options.trim_chunks {
                text.trim().to_string()
            } else {
                text
            };

            if text_trimmed.is_empty() {
                continue;
            }

            // Check if adding this text would exceed chunk size
            let test_chunk = if current_chunk.is_empty() {
                text_trimmed.clone()
            } else {
                format!("{}\n{}", current_chunk, text_trimmed)
            };

            if test_chunk.len() <= self.options.chunk_size {
                // Add to current chunk
                if current_chunk.is_empty() {
                    current_chunk = text_trimmed;
                } else {
                    current_chunk = format!("{}\n{}", current_chunk, text_trimmed);
                }
            } else {
                // Current chunk is full, save it and start new one
                if !current_chunk.is_empty() {
                    chunks.push(current_chunk.clone());
                }

                // If text itself is too large, split it
                if text_trimmed.len() > self.options.chunk_size {
                    let sub_chunks = self.split_large_text(&text_trimmed);
                    chunks.extend(sub_chunks);
                    current_chunk = String::new();
                } else {
                    current_chunk = text_trimmed;
                }
            }
        }

        // Add remaining chunk
        if !current_chunk.is_empty() {
            chunks.push(current_chunk);
        }

        // Apply overlap
        self.apply_overlap(chunks)
    }

    /// Split large text into smaller chunks
    fn split_large_text(&self, text: &str) -> Vec<String> {
        let mut chunks = Vec::new();
        let mut start = 0;

        while start < text.len() {
            let end = (start + self.options.chunk_size).min(text.len());
            let chunk = text[start..end].to_string();
            let trimmed = if self.options.trim_chunks {
                chunk.trim().to_string()
            } else {
                chunk
            };
            if !trimmed.is_empty() {
                chunks.push(trimmed);
            }
            start = end.saturating_sub(self.options.chunk_overlap);
        }

        chunks
    }

    /// Apply overlap between chunks
    fn apply_overlap(&self, chunks: Vec<String>) -> Vec<String> {
        if self.options.chunk_overlap == 0 || chunks.len() <= 1 {
            return chunks;
        }

        let mut overlapped = Vec::new();
        for (i, chunk) in chunks.iter().enumerate() {
            if i == 0 {
                overlapped.push(chunk.clone());
            } else {
                // Add overlap from previous chunk
                let prev_chunk = &chunks[i - 1];
                let overlap_start = prev_chunk.len().saturating_sub(self.options.chunk_overlap);
                let overlap_text = &prev_chunk[overlap_start..];

                let mut new_chunk = String::new();
                if !overlap_text.is_empty() {
                    new_chunk.push_str(overlap_text);
                    new_chunk.push('\n');
                }
                new_chunk.push_str(chunk);
                overlapped.push(new_chunk);
            }
        }

        overlapped
    }
}

#[async_trait]
impl TextSplitter for JsonSplitter {
    async fn split_text(&self, text: &str) -> Result<Vec<String>, TextSplitterError> {
        if text.is_empty() {
            return Ok(vec![]);
        }

        if self.options.chunk_size == 0 {
            return Err(TextSplitterError::InvalidSplitterOptions);
        }

        // Parse JSON
        let json: Value = serde_json::from_str(text)
            .map_err(|e| TextSplitterError::OtherError(format!("JSON parse error: {}", e)))?;

        // Extract elements
        let elements = self.extract_json_elements(&json, "");

        // Split into chunks
        let chunks = self.split_elements_into_chunks(elements);

        Ok(chunks)
    }
}
