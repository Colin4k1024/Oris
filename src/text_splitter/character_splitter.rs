use async_trait::async_trait;

use super::{TextSplitter, TextSplitterError};

/// Configuration for CharacterTextSplitter
#[derive(Debug, Clone)]
pub struct CharacterTextSplitterOptions {
    /// Maximum chunk size (in characters)
    pub chunk_size: usize,
    /// Overlap between chunks (in characters)
    pub chunk_overlap: usize,
    /// Separator character(s) to split on
    pub separator: String,
    /// Whether to trim whitespace from chunks
    pub trim_chunks: bool,
}

impl Default for CharacterTextSplitterOptions {
    fn default() -> Self {
        Self::new()
    }
}

impl CharacterTextSplitterOptions {
    pub fn new() -> Self {
        Self {
            chunk_size: 1000,
            chunk_overlap: 200,
            separator: " ".to_string(),
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

    pub fn with_separator<S: Into<String>>(mut self, separator: S) -> Self {
        self.separator = separator.into();
        self
    }

    pub fn with_trim_chunks(mut self, trim_chunks: bool) -> Self {
        self.trim_chunks = trim_chunks;
        self
    }
}

/// CharacterTextSplitter splits text using a single separator character
///
/// This is a simple splitter that splits text on a specified separator.
/// It's useful when you need a straightforward character-based split.
pub struct CharacterTextSplitter {
    options: CharacterTextSplitterOptions,
}

impl Default for CharacterTextSplitter {
    fn default() -> Self {
        Self::new()
    }
}

impl CharacterTextSplitter {
    /// Create a new CharacterTextSplitter with default options
    pub fn new() -> Self {
        Self::with_options(CharacterTextSplitterOptions::default())
    }

    /// Create a new CharacterTextSplitter with custom options
    pub fn with_options(options: CharacterTextSplitterOptions) -> Self {
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

    /// Set separator
    pub fn with_separator<S: Into<String>>(mut self, separator: S) -> Self {
        self.options.separator = separator.into();
        self
    }

    /// Split text by the separator
    fn split_by_separator(&self, text: &str) -> Vec<String> {
        if text.is_empty() {
            return vec![];
        }

        let parts: Vec<&str> = if self.options.separator.is_empty() {
            // Empty separator means split by character
            return self.split_by_characters(text);
        } else {
            text.split(&self.options.separator).collect()
        };

        let mut chunks = Vec::new();
        let mut current_chunk = String::new();

        for (i, part) in parts.iter().enumerate() {
            let part_with_sep = if i > 0 && !self.options.separator.is_empty() {
                format!("{}{}", self.options.separator, part)
            } else {
                part.to_string()
            };

            // Check if adding this part would exceed chunk size
            let test_chunk = if current_chunk.is_empty() {
                part_with_sep.clone()
            } else {
                format!("{}{}", current_chunk, part_with_sep)
            };

            if test_chunk.len() <= self.options.chunk_size {
                // Add to current chunk
                if current_chunk.is_empty() {
                    current_chunk = part_with_sep;
                } else {
                    current_chunk.push_str(&part_with_sep);
                }
            } else {
                // Current chunk is full, save it and start new one
                if !current_chunk.is_empty() {
                    let trimmed = if self.options.trim_chunks {
                        current_chunk.trim().to_string()
                    } else {
                        current_chunk.clone()
                    };
                    if !trimmed.is_empty() {
                        chunks.push(trimmed);
                    }
                }

                // If part itself is too large, split by characters
                if part.len() > self.options.chunk_size {
                    let sub_chunks = self.split_by_characters(part);
                    chunks.extend(sub_chunks);
                    current_chunk = String::new();
                } else {
                    current_chunk = part_with_sep;
                }
            }
        }

        // Add remaining chunk
        if !current_chunk.is_empty() {
            let trimmed = if self.options.trim_chunks {
                current_chunk.trim().to_string()
            } else {
                current_chunk
            };
            if !trimmed.is_empty() {
                chunks.push(trimmed);
            }
        }

        // Apply overlap
        self.apply_overlap(chunks)
    }

    /// Split text by characters when part is too large
    fn split_by_characters(&self, text: &str) -> Vec<String> {
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
                    if !self.options.separator.is_empty() {
                        new_chunk.push_str(&self.options.separator);
                    }
                }
                new_chunk.push_str(chunk);
                overlapped.push(new_chunk);
            }
        }

        overlapped
    }
}

#[async_trait]
impl TextSplitter for CharacterTextSplitter {
    async fn split_text(&self, text: &str) -> Result<Vec<String>, TextSplitterError> {
        if text.is_empty() {
            return Ok(vec![]);
        }

        if self.options.chunk_size == 0 {
            return Err(TextSplitterError::InvalidSplitterOptions);
        }

        let chunks = self.split_by_separator(text);
        Ok(chunks)
    }
}
