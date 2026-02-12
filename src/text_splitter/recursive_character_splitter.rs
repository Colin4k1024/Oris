use async_trait::async_trait;

use super::{TextSplitter, TextSplitterError};

/// Configuration for RecursiveCharacterTextSplitter
#[derive(Debug, Clone)]
pub struct RecursiveCharacterTextSplitterOptions {
    /// Maximum chunk size (in characters)
    pub chunk_size: usize,
    /// Overlap between chunks (in characters)
    pub chunk_overlap: usize,
    /// List of separators to try in order
    pub separators: Vec<String>,
    /// Whether to trim whitespace from chunks
    pub trim_chunks: bool,
}

impl Default for RecursiveCharacterTextSplitterOptions {
    fn default() -> Self {
        Self::new()
    }
}

impl RecursiveCharacterTextSplitterOptions {
    pub fn new() -> Self {
        Self {
            chunk_size: 1000,
            chunk_overlap: 200,
            separators: vec![
                "\n\n".to_string(),
                "\n".to_string(),
                " ".to_string(),
                "".to_string(),
            ],
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

    pub fn with_separators(mut self, separators: Vec<String>) -> Self {
        self.separators = separators;
        self
    }

    pub fn with_trim_chunks(mut self, trim_chunks: bool) -> Self {
        self.trim_chunks = trim_chunks;
        self
    }
}

/// RecursiveCharacterTextSplitter splits text recursively by trying different separators
///
/// This is the recommended text splitter for most use cases. It attempts to split text
/// on a list of separators in order, trying to keep larger semantic units (paragraphs,
/// sentences, words) intact.
///
/// Default separators: ["\n\n", "\n", " ", ""]
pub struct RecursiveCharacterTextSplitter {
    options: RecursiveCharacterTextSplitterOptions,
}

impl Default for RecursiveCharacterTextSplitter {
    fn default() -> Self {
        Self::new()
    }
}

impl RecursiveCharacterTextSplitter {
    /// Create a new RecursiveCharacterTextSplitter with default options
    pub fn new() -> Self {
        Self::with_options(RecursiveCharacterTextSplitterOptions::default())
    }

    /// Create a new RecursiveCharacterTextSplitter with custom options
    pub fn with_options(options: RecursiveCharacterTextSplitterOptions) -> Self {
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

    /// Set separators
    pub fn with_separators(mut self, separators: Vec<String>) -> Self {
        self.options.separators = separators;
        self
    }

    /// Recursively split text using the separator list
    fn split_text_recursive(&self, text: &str, separators: &[String]) -> Vec<String> {
        if text.is_empty() {
            return vec![];
        }

        // If we've tried all separators, split by character
        if separators.is_empty() {
            return self.split_by_characters(text);
        }

        let separator = &separators[0];
        let remaining_separators = &separators[1..];

        // Split by current separator
        let parts: Vec<&str> = if separator.is_empty() {
            // Empty separator means split by character
            return self.split_by_characters(text);
        } else {
            text.split(separator).collect()
        };

        let mut chunks = Vec::new();
        let mut current_chunk = String::new();

        for part in parts {
            let part_with_sep = if !separator.is_empty() && !current_chunk.is_empty() {
                format!("{}{}", separator, part)
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

                // Try to split the part recursively with remaining separators
                if part.len() > self.options.chunk_size {
                    let sub_chunks = self.split_text_recursive(part, remaining_separators);
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

    /// Split text by characters when no separators work
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
                    new_chunk.push(' ');
                }
                new_chunk.push_str(chunk);
                overlapped.push(new_chunk);
            }
        }

        overlapped
    }
}

#[async_trait]
impl TextSplitter for RecursiveCharacterTextSplitter {
    async fn split_text(&self, text: &str) -> Result<Vec<String>, TextSplitterError> {
        if text.is_empty() {
            return Ok(vec![]);
        }

        if self.options.chunk_size == 0 {
            return Err(TextSplitterError::InvalidSplitterOptions);
        }

        let chunks = self.split_text_recursive(text, &self.options.separators);
        Ok(chunks)
    }
}
